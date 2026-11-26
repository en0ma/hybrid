//! Pure, execution-independent preflight for a bounded linked ASK chain.
//!
//! This module does NOT authenticate Solana accounts or authorize transfers.
//! A future on-chain opcode must verify program ownership, canonical PDAs,
//! custody floors, transaction account packing and per-tick epoch PDAs before
//! acting on the returned plan. No existing opcode accepts this plan as input.
use crate::lazy_pro_rata::{allocate_ask, MakerAllocation};
use crate::{validate_ask_chain, AskOwnerPage, AskPage};

/// Deliberately smaller than a full Solana account/compute limit. These are
/// *planner* caps, not evidence that an executable transaction will fit.
pub const MAX_PLANNED_ASK_PAGES: usize = 2;
pub const MAX_PLANNED_ASK_TICKS: usize = 4;
pub const MAX_PLANNED_ASK_SEATS_PER_TICK: usize = 8;
const FEE_BPS: u128 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskPlanError {
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
pub struct PlannedAskTick {
    pub price_x64: u128,
    pub sqrt_price_x64: u128,
    pub makers: Vec<[u8; 32]>,
    pub weights: Vec<u64>,
    pub filled_base: u64,
    pub quote_proceeds: u64,
    pub quote_fee: u64,
    pub allocations: Vec<MakerAllocation>,
}

impl PlannedAskTick {
    pub fn totals(&self) -> [u64; 6] {
        [
            self.filled_base,
            0,
            0,
            self.quote_proceeds,
            0,
            self.quote_fee,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedAskExecution {
    pub ticks: Vec<PlannedAskTick>,
    pub filled_base: u64,
    pub quote_proceeds: u64,
    pub quote_fee: u64,
}

/// Build a deterministic *read-only* price-by-price ASK plan from canonical
/// ASK pages and their aligned owner sidecars, best price first.
///
/// Reject duplicate makers across the complete chain, bad page links, an
/// overlarge single-price roster and incompatible cached sqrt prices.
/// Never average prices across ticks or allocate quote at a single tick
/// for a fill that crossed multiple prices.
pub fn plan_multilevel_ask(
    pages: &[AskPage],
    sidecars: &[AskOwnerPage],
    requested_base: u64,
    max_quote_in: u64,
) -> Result<PlannedAskExecution, AskPlanError> {
    if pages.is_empty() || pages.len() != sidecars.len() {
        return Err(AskPlanError::InvalidChain);
    }
    if pages.len() > MAX_PLANNED_ASK_PAGES {
        return Err(AskPlanError::TooManyPages);
    }
    if requested_base == 0 || max_quote_in == 0 {
        return Err(AskPlanError::InvalidFill);
    }
    // Public state structs can be fabricated in native callers. Reject
    // impossible lengths before as_slice() or the chain validator indexes.
    if pages.iter().any(|page| page.len() > crate::ASKS_PER_PAGE)
        || sidecars
            .iter()
            .any(|page| page.len() > crate::ASKS_PER_PAGE)
    {
        return Err(AskPlanError::InvalidChain);
    }
    validate_ask_chain(pages).map_err(|_| AskPlanError::InvalidChain)?;
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
            return Err(AskPlanError::InvalidRoster);
        }
        for (entry, maker) in page.as_slice().iter().zip(sidecar.as_slice()) {
            if *maker == [0; 32] || makers_seen.contains(maker) {
                return Err(AskPlanError::InvalidRoster);
            }
            makers_seen.push(*maker);
            if hybrid_engine::validate_limit_ask(hybrid_engine::LimitAsk {
                price_x64: entry.price_x64,
                sqrt_price_x64: entry.sqrt_price_x64,
                base_qty: entry.base_qty,
            })
            .is_err()
            {
                return Err(AskPlanError::InvalidPrice);
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
                return Err(AskPlanError::InvalidPrice);
            }
            index += 1;
        }
        if index - start > MAX_PLANNED_ASK_SEATS_PER_TICK {
            return Err(AskPlanError::TooManySeats);
        }
        if ticks.len() >= MAX_PLANNED_ASK_TICKS {
            return Err(AskPlanError::TooManyTicks);
        }
        let tick_rows = &rows[start..index];
        let weights: Vec<u64> = tick_rows.iter().map(|row| row.2).collect();
        let makers: Vec<[u8; 32]> = tick_rows.iter().map(|row| row.3).collect();
        let capacity = weights
            .iter()
            .try_fold(0u64, |sum, qty| sum.checked_add(*qty))
            .ok_or(AskPlanError::Arithmetic)?;
        let filled_base = remaining.min(capacity);
        let quote_proceeds = hybrid_engine::quote_for_base_at_price(filled_base, price)
            .map_err(|_| AskPlanError::Arithmetic)?;
        let quote_fee = ((u128::from(quote_proceeds) * FEE_BPS) / 10_000) as u64;
        let allocations = allocate_ask(&weights, filled_base, quote_proceeds, quote_fee)
            .map_err(|_| AskPlanError::InvalidFill)?;
        if allocations
            .iter()
            .any(|claim| claim.base_fill > 0 && claim.quote_proceeds == 0)
        {
            return Err(AskPlanError::InvalidFill);
        }
        remaining -= filled_base;
        quote_total = quote_total
            .checked_add(quote_proceeds)
            .ok_or(AskPlanError::Arithmetic)?;
        fee_total = fee_total
            .checked_add(quote_fee)
            .ok_or(AskPlanError::Arithmetic)?;
        ticks.push(PlannedAskTick {
            price_x64: price,
            sqrt_price_x64: sqrt,
            makers,
            weights,
            filled_base,
            quote_proceeds,
            quote_fee,
            allocations,
        });
    }
    if remaining != 0
        || quote_total.checked_add(fee_total).ok_or(AskPlanError::Arithmetic)? > max_quote_in
    {
        return Err(AskPlanError::InvalidFill);
    }
    Ok(PlannedAskExecution {
        ticks,
        filled_base: requested_base,
        quote_proceeds: quote_total,
        quote_fee: fee_total,
    })
}

