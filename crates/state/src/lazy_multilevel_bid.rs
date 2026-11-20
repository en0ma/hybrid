//! Pure, execution-independent preflight for a bounded linked BID chain.
//!
//! This module does NOT authenticate Solana accounts or authorize transfers.
//! A future on-chain opcode must verify program ownership, canonical PDAs,
//! custody floors, transaction account packing and per-tick epoch PDAs before
//! acting on the returned plan. No existing opcode accepts this plan as input.
use crate::lazy_pro_rata::{allocate_bid_at_price, MakerBidAllocation};
use crate::{validate_bid_chain, BidOwnerPage, BidPage};

/// Deliberately smaller than a full Solana account/compute limit. These are
/// *planner* caps, not evidence that an executable transaction will fit.
pub const MAX_PLANNED_BID_PAGES: usize = 2;
pub const MAX_PLANNED_BID_TICKS: usize = 4;
pub const MAX_PLANNED_BID_SEATS_PER_TICK: usize = 16;
const FEE_BPS: u128 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BidPlanError {
    InvalidChain,
    InvalidPrice,
    InvalidRoster,
    TooManyPages,
    TooManyTicks,
    TooManySeats,
    InvalidFill,
    Arithmetic,
}

/// One tick has its own canonical maker rows and independent claim totals.
/// A future opcode must seal a separate immutable epoch for each row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedBidTick {
    pub price_x64: u128,
    pub sqrt_price_x64: u128,
    pub makers: Vec<[u8; 32]>,
    pub weights: Vec<u64>,
    pub filled_base: u64,
    pub quote_paid: u64,
    pub base_fee: u64,
    pub allocations: Vec<MakerBidAllocation>,
}

impl PlannedBidTick {
    pub fn totals(&self) -> [u64; 6] {
        [
            0,
            self.filled_base - self.base_fee,
            self.quote_paid,
            0,
            self.base_fee,
            0,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedBidExecution {
    pub ticks: Vec<PlannedBidTick>,
    pub filled_base: u64,
    pub quote_paid: u64,
    pub base_fee: u64,
}

/// Build a deterministic *read-only* price-by-price BID plan from canonical
/// BID pages and their aligned owner sidecars, best price first.
///
/// Reject duplicate makers across the complete chain, bad page links, an
/// overlarge single-price roster and incompatible cached sqrt prices.
/// Never average prices across ticks or allocate quote at a single tick
/// for a fill that crossed multiple prices.
pub fn plan_multilevel_bid(
    pages: &[BidPage],
    sidecars: &[BidOwnerPage],
    requested_base: u64,
    min_quote_out: u64,
) -> Result<PlannedBidExecution, BidPlanError> {
    if pages.is_empty() || pages.len() != sidecars.len() {
        return Err(BidPlanError::InvalidChain);
    }
    if pages.len() > MAX_PLANNED_BID_PAGES {
        return Err(BidPlanError::TooManyPages);
    }
    if requested_base == 0 || min_quote_out == 0 {
        return Err(BidPlanError::InvalidFill);
    }
    // Public state structs can be fabricated in native callers. Reject
    // impossible lengths before as_slice() or the chain validator indexes.
    if pages.iter().any(|page| page.len() > crate::BIDS_PER_PAGE)
        || sidecars
            .iter()
            .any(|page| page.len() > crate::BIDS_PER_PAGE)
    {
        return Err(BidPlanError::InvalidChain);
    }
    validate_bid_chain(pages).map_err(|_| BidPlanError::InvalidChain)?;
    let mut rows = Vec::new();
    let mut makers_seen = Vec::new();
    for (page, sidecar) in pages.iter().zip(sidecars) {
        if page.is_empty()
            || sidecar.len() != page.len()
            || sidecar.links() != page.links()
            || sidecar.owners[sidecar.len()..]
                .iter()
                .any(|owner| *owner != [0; 32])
        {
            return Err(BidPlanError::InvalidRoster);
        }
        for (entry, maker) in page.as_slice().iter().zip(sidecar.as_slice()) {
            if *maker == [0; 32] || makers_seen.contains(maker) {
                return Err(BidPlanError::InvalidRoster);
            }
            makers_seen.push(*maker);
            if hybrid_engine::validate_limit_ask(hybrid_engine::LimitAsk {
                price_x64: entry.price_x64,
                sqrt_price_x64: entry.sqrt_price_x64,
                base_qty: entry.base_qty,
            })
            .is_err()
            {
                return Err(BidPlanError::InvalidPrice);
            }
            rows.push((
                entry.price_x64,
                entry.sqrt_price_x64,
                entry.base_qty,
                *maker,
            ));
        }
    }

    let mut remaining = requested_base;
    let mut quote_total = 0u64;
    let mut fee_total = 0u64;
    let mut ticks = Vec::new();
    let mut index = 0usize;
    while index < rows.len() && remaining > 0 {
        let price = rows[index].0;
        let sqrt = rows[index].1;
        let start = index;
        while index < rows.len() && rows[index].0 == price {
            if rows[index].1 != sqrt {
                return Err(BidPlanError::InvalidPrice);
            }
            index += 1;
        }
        if index - start > MAX_PLANNED_BID_SEATS_PER_TICK {
            return Err(BidPlanError::TooManySeats);
        }
        if ticks.len() >= MAX_PLANNED_BID_TICKS {
            return Err(BidPlanError::TooManyTicks);
        }
        let tick_rows = &rows[start..index];
        let weights: Vec<u64> = tick_rows.iter().map(|row| row.2).collect();
        let makers: Vec<[u8; 32]> = tick_rows.iter().map(|row| row.3).collect();
        let capacity = weights
            .iter()
            .try_fold(0u64, |sum, qty| sum.checked_add(*qty))
            .ok_or(BidPlanError::Arithmetic)?;
        let filled_base = remaining.min(capacity);
        let base_fee = ((u128::from(filled_base) * FEE_BPS) / 10_000) as u64;
        let quote_paid = hybrid_engine::quote_for_base_at_price(filled_base, price)
            .map_err(|_| BidPlanError::Arithmetic)?;
        let allocations = allocate_bid_at_price(&weights, filled_base, price, base_fee)
            .map_err(|_| BidPlanError::InvalidFill)?;
        remaining -= filled_base;
        quote_total = quote_total
            .checked_add(quote_paid)
            .ok_or(BidPlanError::Arithmetic)?;
        fee_total = fee_total
            .checked_add(base_fee)
            .ok_or(BidPlanError::Arithmetic)?;
        ticks.push(PlannedBidTick {
            price_x64: price,
            sqrt_price_x64: sqrt,
            makers,
            weights,
            filled_base,
            quote_paid,
            base_fee,
            allocations,
        });
    }
    if remaining != 0 || quote_total < min_quote_out {
        return Err(BidPlanError::InvalidFill);
    }
    Ok(PlannedBidExecution {
        ticks,
        filled_base: requested_base,
        quote_paid: quote_total,
        base_fee: fee_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BidEntry, PageLinks};
    use hybrid_engine::Q64;

    fn chain() -> (Vec<BidPage>, Vec<BidOwnerPage>) {
        let mut pages = vec![BidPage::default(), BidPage::default()];
        let mut sidecars = vec![BidOwnerPage::default(), BidOwnerPage::default()];
        for page in 0..2usize {
            let links = PageLinks::new(
                page as u32,
                (page > 0).then_some((page - 1) as u32),
                (page == 0).then_some(1),
            );
            pages[page].set_links(links);
            sidecars[page].set_links(links);
        }
        // A full head page holds two distinct 16-maker ticks; the tail
        // adds one more lower-price tick. All 33 maker keys are unique.
        for index in 0..33usize {
            let page = usize::from(index >= 32);
            let (price, sqrt) = if index < 16 {
                (Q64 * 2, 26_087_635_650_665_564_425u128)
            } else if index < 32 {
                (Q64, Q64)
            } else {
                (Q64 / 2, 13_043_817_825_332_782_213u128)
            };
            let entry = BidEntry {
                price_x64: price,
                sqrt_price_x64: sqrt,
                base_qty: 100,
                sequence: index as u64 + 1,
            };
            pages[page].insert(entry).unwrap();
            sidecars[page].owners[index % 32] = [(index + 1) as u8; 32];
        }
        sidecars[0].len = 32;
        sidecars[1].len = 1;
        (pages, sidecars)
    }

    #[test]
    fn two_linked_pages_price_each_tick_independently_with_exact_conservation() {
        let (pages, sidecars) = chain();
        let plan = plan_multilevel_bid(&pages, &sidecars, 1_650, 3_250).unwrap();
        assert_eq!(plan.ticks.len(), 2);
        assert_eq!(plan.ticks[0].filled_base, 1_600);
        assert_eq!(plan.ticks[0].quote_paid, 3_200);
        assert_eq!(plan.ticks[0].base_fee, 16);
        assert_eq!(plan.ticks[1].filled_base, 50);
        assert_eq!(plan.ticks[1].quote_paid, 50);
        assert_eq!(plan.quote_paid, 3_250);
        assert_eq!(plan.base_fee, 16);
        assert_eq!(
            plan.ticks.iter().map(|tick| tick.totals()[1]).sum::<u64>(),
            1_634
        );
        for tick in &plan.ticks {
            assert_eq!(
                tick.allocations.iter().map(|a| a.quote_spent).sum::<u64>(),
                tick.quote_paid
            );
            assert_eq!(
                tick.allocations
                    .iter()
                    .map(|a| a.base_proceeds + a.base_fee)
                    .sum::<u64>(),
                tick.filled_base
            );
        }
    }

    #[test]
    fn third_tick_uses_its_own_fractional_price_and_separate_totals() {
        let (pages, sidecars) = chain();
        let plan = plan_multilevel_bid(&pages, &sidecars, 3_300, 4_850).unwrap();
        assert_eq!(plan.ticks.len(), 3);
        assert_eq!(plan.ticks[2].price_x64, Q64 / 2);
        assert_eq!(plan.ticks[2].quote_paid, 50);
        assert_eq!(plan.quote_paid, 4_850);
        assert_eq!(plan.ticks[2].totals(), [0, 100, 50, 0, 0, 0]);
    }

    #[test]
    fn malformed_chains_duplicate_owners_and_impossible_fills_fail_closed() {
        let (pages, sidecars) = chain();
        assert_eq!(
            plan_multilevel_bid(&pages, &sidecars, 3_301, 1),
            Err(BidPlanError::InvalidFill)
        );
        assert_eq!(
            plan_multilevel_bid(&pages, &sidecars, 1_650, 3_251),
            Err(BidPlanError::InvalidFill)
        );
        let mut bad = sidecars.clone();
        bad[1].owners[0] = bad[0].owners[0];
        assert_eq!(
            plan_multilevel_bid(&pages, &bad, 1_650, 3_250),
            Err(BidPlanError::InvalidRoster)
        );
        let mut bad = pages.clone();
        bad[0].set_links(PageLinks::new(0, None, None));
        assert_eq!(
            plan_multilevel_bid(&bad, &sidecars, 1_650, 3_250),
            Err(BidPlanError::InvalidChain)
        );
        let mut bad = sidecars.clone();
        bad[0].owners[31] = [0; 32];
        assert_eq!(
            plan_multilevel_bid(&pages, &bad, 1_650, 3_250),
            Err(BidPlanError::InvalidRoster)
        );
    }

    #[test]
    fn per_tick_seat_cap_cannot_be_bypassed_by_splitting_pages() {
        let (mut pages, sidecars) = chain();
        pages[0].entries[16].price_x64 = Q64 * 2;
        pages[0].entries[16].sqrt_price_x64 = pages[0].entries[0].sqrt_price_x64;
        assert_eq!(
            plan_multilevel_bid(&pages, &sidecars, 1_650, 1),
            Err(BidPlanError::TooManySeats)
        );
    }
}
