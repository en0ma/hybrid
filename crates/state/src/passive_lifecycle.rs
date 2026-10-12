//! Checked, position-attributed passive settlement ledger.
//!
//! This module is an accounting model, NOT a token-transfer authorization.
//! Its transitions must be executed atomically alongside authenticated SPL
//! transfers, market price updates, and persistent position-state changes.
//! It cannot be substituted for the existing principal-only on-chain close.
use crate::passive::{PassiveAccountingError, PassivePool};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiquidityClaim {
    pub liquidity: u128,
    pub base: u64,
    pub quote: u64,
    pub base_fees: u64,
    pub quote_fees: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trade {
    Buy {
        quote_in: u64,
        base_out: u64,
        quote_fee: u64,
    },
    Sell {
        base_in: u64,
        quote_out: u64,
        base_fee: u64,
    },
}

fn split(amount: u64, weights: &[u128]) -> Result<Vec<u64>, PassiveAccountingError> {
    if weights.is_empty() || weights.contains(&0) {
        return Err(PassiveAccountingError::InvalidPosition);
    }
    let total = weights.iter().try_fold(0u128, |sum, weight| {
        sum.checked_add(*weight).ok_or(PassiveAccountingError::Overflow)
    })?;
    // Division first avoids multiplying u64 amounts by potentially huge
    // liquidity values. The final residual goes to the last position, so
    // shares sum exactly to the transfer quantity with no invented atoms.
    let mut remaining = amount;
    let mut shares = Vec::with_capacity(weights.len());
    let mut remaining_weight = total;
    for weight in weights {
        let amount = u128::from(remaining)
            .checked_mul(*weight)
            .ok_or(PassiveAccountingError::Overflow)?;
        let share = u64::try_from(amount / remaining_weight)
            .map_err(|_| PassiveAccountingError::Overflow)?;
        shares.push(share);
        remaining -= share;
        remaining_weight -= weight;
    }
    if let Some(last) = shares.last_mut() {
        *last = last.checked_add(remaining).ok_or(PassiveAccountingError::Overflow)?;
    }
    Ok(shares)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassiveLedger {
    pub pool: PassivePool,
    pub claims: Vec<LiquidityClaim>,
}

impl PassiveLedger {
    pub fn verify(&self) -> Result<(), PassiveAccountingError> {
        let mut base = 0u64;
        let mut quote = 0u64;
        let mut base_fees = 0u64;
        let mut quote_fees = 0u64;
        let mut liquidity = 0u128;
        for claim in &self.claims {
            if claim.liquidity == 0 {
                return Err(PassiveAccountingError::InvalidPosition);
            }
            base = base.checked_add(claim.base).ok_or(PassiveAccountingError::Overflow)?;
            quote = quote.checked_add(claim.quote).ok_or(PassiveAccountingError::Overflow)?;
            base_fees = base_fees
                .checked_add(claim.base_fees)
                .ok_or(PassiveAccountingError::Overflow)?;
            quote_fees = quote_fees
                .checked_add(claim.quote_fees)
                .ok_or(PassiveAccountingError::Overflow)?;
            liquidity = liquidity
                .checked_add(claim.liquidity)
                .ok_or(PassiveAccountingError::Overflow)?;
        }
        if base != self.pool.base_reserve
            || quote != self.pool.quote_reserve
            || base_fees != self.pool.accrued_base_fees
            || quote_fees != self.pool.accrued_quote_fees
            || liquidity != self.pool.total_liquidity
        {
            return Err(PassiveAccountingError::InvariantViolation);
        }
        Ok(())
    }

    /// Apply all debits and fee allocations on a copy. On error, no mutation
    /// escapes. Every position shares the transfer exactly by liquidity,
    /// with a deterministic remainder assigned to the last claim.
    pub fn settle(&mut self, trade: Trade) -> Result<(), PassiveAccountingError> {
        self.verify()?;
        let mut next = self.clone();
        let weights: Vec<_> = next.claims.iter().map(|p| p.liquidity).collect();
        match trade {
            Trade::Buy {
                quote_in,
                base_out,
                quote_fee,
            } => {
                next.pool.buy(quote_in, base_out, quote_fee)?;
                let out = split(base_out, &weights)?;
                let paid = split(quote_in - quote_fee, &weights)?;
                let fees = split(quote_fee, &weights)?;
                for (i, claim) in next.claims.iter_mut().enumerate() {
                    claim.base = claim.base
                        .checked_sub(out[i])
                        .ok_or(PassiveAccountingError::InsufficientReserves)?;
                    claim.quote = claim.quote
                        .checked_add(paid[i])
                        .ok_or(PassiveAccountingError::Overflow)?;
                    claim.quote_fees = claim.quote_fees
                        .checked_add(fees[i])
                        .ok_or(PassiveAccountingError::Overflow)?;
                }
            }
            Trade::Sell {
                base_in,
                quote_out,
                base_fee,
            } => {
                next.pool.sell(base_in, quote_out, base_fee)?;
                let out = split(quote_out, &weights)?;
                let paid = split(base_in - base_fee, &weights)?;
                let fees = split(base_fee, &weights)?;
                for (i, claim) in next.claims.iter_mut().enumerate() {
                    claim.quote = claim.quote
                        .checked_sub(out[i])
                        .ok_or(PassiveAccountingError::InsufficientReserves)?;
                    claim.base = claim.base
                        .checked_add(paid[i])
                        .ok_or(PassiveAccountingError::Overflow)?;
                    claim.base_fees = claim.base_fees
                        .checked_add(fees[i])
                        .ok_or(PassiveAccountingError::Overflow)?;
                }
            }
        }
        next.verify()?;
        *self = next;
        Ok(())
    }

    /// Remove current assets and attributable fees, never the historical
    /// original deposit quantities. This represents a complete withdrawal.
    pub fn redeem(&mut self, index: usize) -> Result<(u64, u64), PassiveAccountingError> {
        self.verify()?;
        let mut next = self.clone();
        if index >= next.claims.len() {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        let claim = next.claims.remove(index);
        next.pool.base_reserve = next.pool.base_reserve
            .checked_sub(claim.base)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.quote_reserve = next.pool.quote_reserve
            .checked_sub(claim.quote)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.accrued_base_fees = next.pool.accrued_base_fees
            .checked_sub(claim.base_fees)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.accrued_quote_fees = next.pool.accrued_quote_fees
            .checked_sub(claim.quote_fees)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        next.pool.total_liquidity = next.pool.total_liquidity
            .checked_sub(claim.liquidity)
            .ok_or(PassiveAccountingError::InsufficientLiquidity)?;
        let base = claim.base.checked_add(claim.base_fees)
            .ok_or(PassiveAccountingError::Overflow)?;
        let quote = claim.quote.checked_add(claim.quote_fees)
            .ok_or(PassiveAccountingError::Overflow)?;
        next.verify()?;
        *self = next;
        Ok((base, quote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funded() -> PassiveLedger {
        PassiveLedger {
            pool: PassivePool {
                base_reserve: 100,
                quote_reserve: 100,
                total_liquidity: 10,
                ..PassivePool::default()
            },
            claims: vec![
                LiquidityClaim {
                    liquidity: 4,
                    base: 40,
                    quote: 40,
                    ..LiquidityClaim::default()
                },
                LiquidityClaim {
                    liquidity: 6,
                    base: 60,
                    quote: 60,
                    ..LiquidityClaim::default()
                },
            ],
        }
    }

    #[test]
    fn funded_buy_sell_fee_redemption_conserves_all_atoms() {
        let mut ledger = funded();
        ledger.settle(Trade::Buy {
            quote_in: 23,
            base_out: 10,
            quote_fee: 3,
        }).unwrap();
        ledger.verify().unwrap();
        assert_eq!(ledger.pool.accrued_quote_fees, 3);
        ledger.settle(Trade::Sell {
            base_in: 13,
            quote_out: 7,
            base_fee: 2,
        }).unwrap();
        ledger.verify().unwrap();
        assert_eq!(ledger.pool.accrued_base_fees, 2);
        let before = (
            ledger.pool.base_reserve + ledger.pool.accrued_base_fees,
            ledger.pool.quote_reserve + ledger.pool.accrued_quote_fees,
        );
        let a = ledger.redeem(0).unwrap();
        let b = ledger.redeem(0).unwrap();
        assert_eq!(a.0 + b.0, before.0);
        assert_eq!(a.1 + b.1, before.1);
        assert_eq!(ledger.pool, PassivePool::default());
    }

    #[test]
    fn undercollateralized_position_rolls_back_entire_trade() {
        let mut ledger = funded();
        let original = ledger.clone();
        assert_eq!(
            ledger.settle(Trade::Buy {
                quote_in: 10,
                base_out: 99,
                quote_fee: 1,
            }),
            Err(PassiveAccountingError::InsufficientReserves)
        );
        assert_eq!(ledger, original);
    }

    #[test]
    fn redemption_uses_post_trade_inventory_not_original_deposit() {
        let mut ledger = funded();
        ledger.settle(Trade::Buy {
            quote_in: 21,
            base_out: 10,
            quote_fee: 1,
        }).unwrap();
        let first = ledger.redeem(0).unwrap();
        assert_ne!(first, (40, 40));
        ledger.verify().unwrap();
    }

    #[test]
    fn inconsistent_claims_fail_closed() {
        let mut ledger = funded();
        ledger.claims[0].base += 1;
        assert_eq!(ledger.settle(Trade::Buy {
            quote_in: 10,
            base_out: 1,
            quote_fee: 1,
        }), Err(PassiveAccountingError::InvariantViolation));
    }
}
