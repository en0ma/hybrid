//! Checked, single-segment passive quote + LP accounting transition.
//!
//! This remains a PURE state model. It does not authenticate Solana accounts,
//! transfer SPL tokens, persist claims or permit execution on-chain.
//! Never use this as a substitute for the required CPI and PDA checks.
use crate::passive::PassiveAccountingError;
use crate::passive_segments::{SegmentTrade, SegmentedPool};
use hybrid_engine::{
    quote_base_in_for_quote_out, quote_quote_in_for_base_out, range_position::active_liquidity,
    PassiveState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    BuyBase,
    SellBase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricedSegment {
    pub gross_input: u64,
    pub fee: u64,
    pub output: u64,
    pub next_sqrt_price_x64: u128,
}

fn fail() -> PassiveAccountingError {
    PassiveAccountingError::InvalidPosition
}

fn fee_ceil(gross: u64, fee_bps: u16) -> Result<u64, PassiveAccountingError> {
    if fee_bps >= 10_000 {
        return Err(fail());
    }
    let product = u128::from(gross)
        .checked_mul(u128::from(fee_bps))
        .ok_or(PassiveAccountingError::Overflow)?;
    u64::try_from(product.div_ceil(10_000)).map_err(|_| PassiveAccountingError::Overflow)
}

/// Price one net-input trade and distribute its assets/fees to the LPs whose
/// ranges are active throughout the segment. Reject *all* boundary crossings:
/// a real multi-range trade must be broken into separately priced segments.
///
/// The caller must atomically authenticate the account graph, transfer SPL
/// tokens, update the persisted market price and LP claims, and verify vault
/// coverage. This pure model does none of those things.
pub fn apply_priced_segment(
    book: &mut SegmentedPool,
    sqrt_price_x64: &mut u128,
    direction: Direction,
    gross_input: u64,
    fee_bps: u16,
    minimum_output: u64,
) -> Result<PricedSegment, PassiveAccountingError> {
    book.verify()?;
    if gross_input == 0 || *sqrt_price_x64 == 0 {
        return Err(fail());
    }
    let fee = fee_ceil(gross_input, fee_bps)?;
    let net_input = gross_input.checked_sub(fee).ok_or(fail())?;
    if net_input == 0 {
        return Err(fail());
    }

    let ranges: Vec<_> = book.positions.iter().map(|p| p.range).collect();
    let liquidity = active_liquidity(&ranges, *sqrt_price_x64)
        .map_err(|_| PassiveAccountingError::InsufficientLiquidity)?;
    if liquidity == 0 {
        return Err(PassiveAccountingError::InsufficientLiquidity);
    }

    let state = PassiveState {
        sqrt_price_x64: *sqrt_price_x64,
        liquidity,
    };
    let quote = match direction {
        Direction::BuyBase => quote_quote_in_for_base_out(state, net_input),
        Direction::SellBase => quote_base_in_for_quote_out(state, net_input),
    }
    .map_err(|_| fail())?;
    if quote.amount_out == 0 || quote.amount_out < minimum_output {
        return Err(PassiveAccountingError::InsufficientReserves);
    }
    let old = *sqrt_price_x64;
    let next = quote.next_sqrt_price_x64;
    if next == 0 || next == old {
        return Err(fail());
    }
    // A boundary may be crossed even if both endpoints are outside a range.
    // Reject every boundary traversed, including one reached at the endpoint.
    for p in &book.positions {
        let lower = p.range.lower_sqrt_price_x64;
        let upper = p.range.upper_sqrt_price_x64;
        let crossed = if next > old {
            (lower > old && lower <= next) || (upper > old && upper <= next)
        } else {
            (lower < old && lower >= next) || (upper < old && upper >= next)
        };
        if crossed {
            return Err(fail());
        }
    }

    let trade = match direction {
        Direction::BuyBase => SegmentTrade::Buy {
            quote_in: gross_input,
            base_out: quote.amount_out,
            quote_fee: fee,
        },
        Direction::SellBase => SegmentTrade::Sell {
            base_in: gross_input,
            quote_out: quote.amount_out,
            base_fee: fee,
        },
    };
    // The segmented model applies to a clone and only commits on success.
    book.apply_segment(old, trade)?;
    *sqrt_price_x64 = next;
    Ok(PricedSegment {
        gross_input,
        fee,
        output: quote.amount_out,
        next_sqrt_price_x64: next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passive::PassivePool;
    use crate::passive_segments::RangeClaim;
    use hybrid_engine::{range_position::RangePosition, Q64};

    fn pool() -> SegmentedPool {
        SegmentedPool {
            pool: PassivePool {
                base_reserve: 100_000,
                quote_reserve: 100_000,
                total_liquidity: 1_000,
                ..PassivePool::default()
            },
            positions: vec![RangeClaim {
                range: RangePosition {
                    lower_sqrt_price_x64: Q64 / 2,
                    upper_sqrt_price_x64: Q64 * 2,
                    liquidity: 1_000,
                },
                base: 100_000,
                quote: 100_000,
                base_fees: 0,
                quote_fees: 0,
            }],
        }
    }

    #[test]
    fn buy_and_sell_price_and_attribute_fees_before_redemption() {
        let mut book = pool();
        let mut price = Q64;
        let buy =
            apply_priced_segment(&mut book, &mut price, Direction::BuyBase, 101, 100, 1).unwrap();
        assert_eq!(buy.fee, 2);
        assert_eq!(book.pool.accrued_quote_fees, 2);
        assert!(price > Q64);
        let sell =
            apply_priced_segment(&mut book, &mut price, Direction::SellBase, 101, 100, 1).unwrap();
        assert_eq!(sell.fee, 2);
        assert_eq!(book.pool.accrued_base_fees, 2);
        book.verify().unwrap();
        let owed_base = book.pool.base_reserve + book.pool.accrued_base_fees;
        let owed_quote = book.pool.quote_reserve + book.pool.accrued_quote_fees;
        assert_eq!(book.redeem(0), Ok((owed_base, owed_quote)));
        assert_eq!(book.pool, PassivePool::default());
    }

    #[test]
    fn overlapping_lps_receive_exact_inventory_and_fee_claims() {
        let mut book = pool();
        book.positions[0].range.liquidity = 500;
        book.positions[0].base = 50_000;
        book.positions[0].quote = 50_000;
        book.positions.push(RangeClaim {
            range: RangePosition {
                lower_sqrt_price_x64: Q64 / 2,
                upper_sqrt_price_x64: Q64 * 2,
                liquidity: 500,
            },
            base: 50_000,
            quote: 50_000,
            base_fees: 0,
            quote_fees: 0,
        });
        book.verify().unwrap();
        let mut price = Q64;
        let before = book.clone();
        let buy = apply_priced_segment(
            &mut book,
            &mut price,
            Direction::BuyBase,
            101,
            100,
            1,
        )
        .unwrap();
        assert_eq!(buy.fee, 2);
        assert_eq!(book.pool.accrued_quote_fees, 2);
        assert_eq!(
            book.positions.iter().map(|p| p.base_fees).sum::<u64>(),
            0
        );
        assert_eq!(
            book.positions.iter().map(|p| p.quote_fees).sum::<u64>(),
            2
        );
        assert_ne!(book.positions[0].base, before.positions[0].base);
        assert_ne!(book.positions[1].base, before.positions[1].base);
        book.verify().unwrap();
        let expected_base = book.pool.base_reserve + book.pool.accrued_base_fees;
        let expected_quote = book.pool.quote_reserve + book.pool.accrued_quote_fees;
        let first = book.redeem(0).unwrap();
        let second = book.redeem(0).unwrap();
        assert_eq!(first.0 + second.0, expected_base);
        assert_eq!(first.1 + second.1, expected_quote);
        assert_eq!(book.pool, PassivePool::default());
    }

    #[test]
    fn sell_crossing_lower_range_boundary_is_atomic() {
        let mut book = pool();
        let original = book.clone();
        let mut price = Q64;
        assert!(
            apply_priced_segment(
                &mut book,
                &mut price,
                Direction::SellBase,
                1_500,
                0,
                1,
            )
            .is_err()
        );
        assert_eq!(book, original);
        assert_eq!(price, Q64);
    }

    #[test]
    fn failed_slippage_and_boundary_check_roll_back_all_state() {
        let mut book = pool();
        let old = book.clone();
        let mut price = Q64;
        assert!(
            apply_priced_segment(&mut book, &mut price, Direction::BuyBase, 100, 0, u64::MAX)
                .is_err()
        );
        assert_eq!(book, old);
        assert_eq!(price, Q64);

        assert!(
            apply_priced_segment(&mut book, &mut price, Direction::BuyBase, 1_500, 0, 1).is_err()
        );
        assert_eq!(book, old);
        assert_eq!(price, Q64);
    }

    #[test]
    fn invalid_fee_and_insufficient_reserves_are_atomic() {
        let mut book = pool();
        let initial = book.clone();
        let mut price = Q64;
        assert!(
            apply_priced_segment(&mut book, &mut price, Direction::BuyBase, 100, 10_000, 1)
                .is_err()
        );
        assert_eq!(book, initial);
        assert_eq!(price, Q64);
        book.pool.base_reserve = 1;
        book.positions[0].base = 1;
        let original = book.clone();
        assert!(
            apply_priced_segment(&mut book, &mut price, Direction::BuyBase, 100, 0, 1).is_err()
        );
        assert_eq!(book, original);
    }
}
