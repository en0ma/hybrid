//! Range-segmented passive LP accounting model.
//!
//! This is not an on-chain settlement authorization. The executable processor
//! must validate all position PDAs, vaults, market sequence and token transfers
//! in one transaction before persisting any result.
//! Liquidity at the upper range boundary is excluded, at the lower included.

use crate::passive::{PassiveAccountingError, PassivePool};
use hybrid_engine::range_position::RangePosition;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeClaim {
    pub range: RangePosition,
    pub base: u64,
    pub quote: u64,
    pub base_fees: u64,
    pub quote_fees: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentTrade {
    Buy { quote_in: u64, base_out: u64, quote_fee: u64 },
    Sell { base_in: u64, quote_out: u64, base_fee: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentedPool {
    pub pool: PassivePool,
    pub positions: Vec<RangeClaim>,
}

/// floor(amount * numerator / denominator) without a potentially overflowing
/// amount * numerator intermediate. Checked quotient-and-remainder decomposition
/// also supports liquidity denominators above u64::MAX.
fn proportion(amount: u64, numerator: u128, denominator: u128) -> Result<u64, PassiveAccountingError> {
    if denominator == 0 || numerator > denominator {
        return Err(PassiveAccountingError::InvalidPosition);
    }
    if let Some(product) = u128::from(amount).checked_mul(numerator) {
        return u64::try_from(product / denominator)
            .map_err(|_| PassiveAccountingError::Overflow);
    }
    // Long division of the 192-bit product, keeping the remainder strictly
    // below denominator. Subtraction-based modular addition cannot overflow.
    let mut quotient = 0u128;
    let mut remainder = 0u128;
    for bit in (0..64).rev() {
        quotient = quotient.checked_mul(2).ok_or(PassiveAccountingError::Overflow)?;
        let twice = denominator - remainder;
        if remainder >= twice {
            remainder -= twice;
            quotient += 1;
        } else {
            remainder *= 2;
        }
        if (amount >> bit) & 1 == 1 {
            let gap = denominator - numerator;
            if remainder >= gap {
                remainder -= gap;
                quotient += 1;
            } else {
                remainder += numerator;
            }
        }
    }
    u64::try_from(quotient).map_err(|_| PassiveAccountingError::Overflow)
}

impl SegmentedPool {
    pub fn verify(&self) -> Result<(), PassiveAccountingError> {
        let mut total = PassivePool::default();
        for p in &self.positions {
            p.range.validate().map_err(|_| PassiveAccountingError::InvalidPosition)?;
            total.total_liquidity = total.total_liquidity.checked_add(p.range.liquidity)
                .ok_or(PassiveAccountingError::Overflow)?;
            total.base_reserve = total.base_reserve.checked_add(p.base)
                .ok_or(PassiveAccountingError::Overflow)?;
            total.quote_reserve = total.quote_reserve.checked_add(p.quote)
                .ok_or(PassiveAccountingError::Overflow)?;
            total.accrued_base_fees = total.accrued_base_fees.checked_add(p.base_fees)
                .ok_or(PassiveAccountingError::Overflow)?;
            total.accrued_quote_fees = total.accrued_quote_fees.checked_add(p.quote_fees)
                .ok_or(PassiveAccountingError::Overflow)?;
        }
        if total != self.pool {
            return Err(PassiveAccountingError::InvariantViolation);
        }
        Ok(())
    }

    /// Settle one price segment at a fixed active-liquidity set. Callers must
    /// split price movement at EVERY crossed boundary and use the corresponding
    /// segment's input/output and fee amounts, rather than sending a whole
    /// multi-boundary swap here.
    pub fn apply_segment(&mut self, sqrt_price_x64: u128, trade: SegmentTrade)
        -> Result<(), PassiveAccountingError>
    {
        self.verify()?;
        let mut next = self.clone();
        let mut active = Vec::new();
        let mut weight = 0u128;
        for (index, p) in next.positions.iter().enumerate() {
            if p.range.active_at(sqrt_price_x64)
                .map_err(|_| PassiveAccountingError::InvalidPosition)?
            {
                weight = weight.checked_add(p.range.liquidity)
                    .ok_or(PassiveAccountingError::Overflow)?;
                active.push(index);
            }
        }
        if weight == 0 {
            return Err(PassiveAccountingError::InsufficientLiquidity);
        }
        let (debit, credit, fee, buy) = match trade {
            SegmentTrade::Buy { quote_in, base_out, quote_fee } => {
                next.pool.buy(quote_in, base_out, quote_fee)?;
                (base_out, quote_in - quote_fee, quote_fee, true)
            }
            SegmentTrade::Sell { base_in, quote_out, base_fee } => {
                next.pool.sell(base_in, quote_out, base_fee)?;
                (quote_out, base_in - base_fee, base_fee, false)
            }
        };
        let mut debit_left = debit;
        let mut credit_left = credit;
        let mut fee_left = fee;
        let mut weight_left = weight;
        for (step, index) in active.iter().enumerate() {
            let p = &mut next.positions[*index];
            let (d, c, f) = if step + 1 == active.len() {
                (debit_left, credit_left, fee_left)
            } else {
                (
                    proportion(debit_left, p.range.liquidity, weight_left)?,
                    proportion(credit_left, p.range.liquidity, weight_left)?,
                    proportion(fee_left, p.range.liquidity, weight_left)?,
                )
            };
            debit_left -= d;
            credit_left -= c;
            fee_left -= f;
            weight_left -= p.range.liquidity;
            if buy {
                p.base = p.base.checked_sub(d).ok_or(PassiveAccountingError::InsufficientReserves)?;
                p.quote = p.quote.checked_add(c).ok_or(PassiveAccountingError::Overflow)?;
                p.quote_fees = p.quote_fees.checked_add(f).ok_or(PassiveAccountingError::Overflow)?;
            } else {
                p.quote = p.quote.checked_sub(d).ok_or(PassiveAccountingError::InsufficientReserves)?;
                p.base = p.base.checked_add(c).ok_or(PassiveAccountingError::Overflow)?;
                p.base_fees = p.base_fees.checked_add(f).ok_or(PassiveAccountingError::Overflow)?;
            }
        }
        next.verify()?;
        *self = next;
        Ok(())
    }

    pub fn redeem(&mut self, index: usize) -> Result<(u64, u64), PassiveAccountingError> {
        self.verify()?;
        let mut next = self.clone();
        if index >= next.positions.len() {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        let p = next.positions.remove(index);
        next.pool.base_reserve = next.pool.base_reserve.checked_sub(p.base)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.quote_reserve = next.pool.quote_reserve.checked_sub(p.quote)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.accrued_base_fees = next.pool.accrued_base_fees.checked_sub(p.base_fees)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.accrued_quote_fees = next.pool.accrued_quote_fees.checked_sub(p.quote_fees)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.total_liquidity = next.pool.total_liquidity.checked_sub(p.range.liquidity)
            .ok_or(PassiveAccountingError::InsufficientLiquidity)?;
        let base = p.base.checked_add(p.base_fees).ok_or(PassiveAccountingError::Overflow)?;
        let quote = p.quote.checked_add(p.quote_fees).ok_or(PassiveAccountingError::Overflow)?;
        next.verify()?;
        *self = next;
        Ok((base, quote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hybrid_engine::Q64;

    fn fixture() -> SegmentedPool {
        SegmentedPool {
            pool: PassivePool {
                base_reserve: 200,
                quote_reserve: 200,
                total_liquidity: 20,
                ..PassivePool::default()
            },
            positions: vec![
                RangeClaim {
                    range: RangePosition {
                        lower_sqrt_price_x64: Q64 / 2,
                        upper_sqrt_price_x64: Q64 * 2,
                        liquidity: 10,
                    },
                    base: 100,
                    quote: 100,
                    base_fees: 0,
                    quote_fees: 0,
                },
                RangeClaim {
                    range: RangePosition {
                        lower_sqrt_price_x64: Q64 * 2,
                        upper_sqrt_price_x64: Q64 * 4,
                        liquidity: 10,
                    },
                    base: 100,
                    quote: 100,
                    base_fees: 0,
                    quote_fees: 0,
                },
            ],
        }
    }

    #[test]
    fn disjoint_ranges_never_charge_inactive_lp() {
        let mut book = fixture();
        let inactive = book.positions[1];
        book.apply_segment(Q64, SegmentTrade::Buy {
            quote_in: 21, base_out: 10, quote_fee: 1
        }).unwrap();
        assert_eq!(book.positions[1], inactive);
        assert_eq!(book.positions[0].quote_fees, 1);
        book.apply_segment(Q64 * 2, SegmentTrade::Sell {
            base_in: 21, quote_out: 10, base_fee: 1
        }).unwrap();
        assert_eq!(book.positions[0].base_fees, 0);
        assert_eq!(book.positions[1].base_fees, 1);
        let a = book.redeem(0).unwrap();
        let b = book.redeem(0).unwrap();
        assert_eq!(a.0 + b.0, 210);
        assert_eq!(a.1 + b.1, 210);
        assert_eq!(book.pool, PassivePool::default());
    }

    #[test]
    fn overflow_free_full_precision_single_share() {
        assert_eq!(proportion(u64::MAX, u128::MAX, u128::MAX), Ok(u64::MAX));
        assert_eq!(proportion(u64::MAX, 1, u128::MAX), Ok(0));
    }

    #[test]
    fn out_of_range_or_insolvent_trade_does_not_mutate() {
        let mut book = fixture();
        let original = book.clone();
        assert_eq!(book.apply_segment(Q64 / 4, SegmentTrade::Buy {
            quote_in: 10, base_out: 1, quote_fee: 1
        }), Err(PassiveAccountingError::InsufficientLiquidity));
        assert_eq!(book, original);
        assert_eq!(book.apply_segment(Q64, SegmentTrade::Buy {
            quote_in: 10, base_out: 101, quote_fee: 1
        }), Err(PassiveAccountingError::InsufficientReserves));
        assert_eq!(book, original);
    }
}
