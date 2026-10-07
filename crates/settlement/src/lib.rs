#![forbid(unsafe_code)]

use hybrid_engine::ActiveFill;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MakerBalance {
    pub base_free: u64,
    pub base_locked: u64,
    pub quote_free: u64,
    pub quote_locked: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementError {
    Overflow,
    InsufficientBase,
    InsufficientQuote,
    InvalidFill,
    BalanceIndexOutOfBounds,
}

impl MakerBalance {
    pub fn reserve_ask(&mut self, base_qty: u64) -> Result<(), SettlementError> {
        if self.base_free < base_qty {
            return Err(SettlementError::InsufficientBase);
        }
        self.base_free -= base_qty;
        self.base_locked = self
            .base_locked
            .checked_add(base_qty)
            .ok_or(SettlementError::Overflow)?;
        Ok(())
    }

    pub fn release_ask(&mut self, base_qty: u64) -> Result<(), SettlementError> {
        if self.base_locked < base_qty {
            return Err(SettlementError::InsufficientBase);
        }
        self.base_locked -= base_qty;
        self.base_free = self
            .base_free
            .checked_add(base_qty)
            .ok_or(SettlementError::Overflow)?;
        Ok(())
    }

    pub fn reserve_bid(&mut self, quote_qty: u64) -> Result<(), SettlementError> {
        if self.quote_free < quote_qty {
            return Err(SettlementError::InsufficientQuote);
        }
        self.quote_free -= quote_qty;
        self.quote_locked = self
            .quote_locked
            .checked_add(quote_qty)
            .ok_or(SettlementError::Overflow)?;
        Ok(())
    }

    pub fn release_bid(&mut self, quote_qty: u64) -> Result<(), SettlementError> {
        if self.quote_locked < quote_qty {
            return Err(SettlementError::InsufficientQuote);
        }
        self.quote_locked -= quote_qty;
        self.quote_free = self
            .quote_free
            .checked_add(quote_qty)
            .ok_or(SettlementError::Overflow)?;
        Ok(())
    }

    pub fn settle_ask_fill(
        &mut self,
        base_qty: u64,
        quote_qty: u64,
    ) -> Result<(), SettlementError> {
        if base_qty == 0 || quote_qty == 0 {
            return Err(SettlementError::InvalidFill);
        }
        if self.base_locked < base_qty {
            return Err(SettlementError::InsufficientBase);
        }
        let next_quote = self
            .quote_free
            .checked_add(quote_qty)
            .ok_or(SettlementError::Overflow)?;
        self.base_locked -= base_qty;
        self.quote_free = next_quote;
        Ok(())
    }

    pub fn settle_bid_fill(
        &mut self,
        base_qty: u64,
        quote_qty: u64,
    ) -> Result<(), SettlementError> {
        if base_qty == 0 || quote_qty == 0 {
            return Err(SettlementError::InvalidFill);
        }
        if self.quote_locked < quote_qty {
            return Err(SettlementError::InsufficientQuote);
        }
        let next_base = self
            .base_free
            .checked_add(base_qty)
            .ok_or(SettlementError::Overflow)?;
        self.quote_locked -= quote_qty;
        self.base_free = next_base;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveSettlementTotals {
    pub base_to_taker: u64,
    pub quote_from_taker: u64,
}

pub fn settle_buy_active_fills(
    fills: &[ActiveFill],
    ask_owner_balance_indexes: &[u16],
    maker_balances: &mut [MakerBalance],
) -> Result<ActiveSettlementTotals, SettlementError> {
    if ask_owner_balance_indexes.len() < fills.len() {
        return Err(SettlementError::BalanceIndexOutOfBounds);
    }

    let mut totals = ActiveSettlementTotals::default();

    for (fill, balance_index) in fills.iter().zip(ask_owner_balance_indexes.iter()) {
        if fill.base_qty == 0 || fill.quote_qty == 0 {
            return Err(SettlementError::InvalidFill);
        }
        let balance = maker_balances
            .get_mut(usize::from(*balance_index))
            .ok_or(SettlementError::BalanceIndexOutOfBounds)?;

        balance.settle_ask_fill(fill.base_qty, fill.quote_qty)?;
        totals.base_to_taker = totals
            .base_to_taker
            .checked_add(fill.base_qty)
            .ok_or(SettlementError::Overflow)?;
        totals.quote_from_taker = totals
            .quote_from_taker
            .checked_add(fill.quote_qty)
            .ok_or(SettlementError::Overflow)?;
    }

    Ok(totals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(base_qty: u64, quote_qty: u64) -> ActiveFill {
        ActiveFill {
            ask_index: 0,
            base_qty,
            quote_qty,
        }
    }

    #[test]
    fn ask_reservation_release_and_fill_conserve_assets() {
        let mut balance = MakerBalance {
            base_free: 1_000,
            base_locked: 0,
            quote_free: 50,
            quote_locked: 0,
        };

        balance.reserve_ask(600).unwrap();
        assert_eq!(
            balance,
            MakerBalance {
                base_free: 400,
                base_locked: 600,
                quote_free: 50,
                quote_locked: 0,
            }
        );

        balance.settle_ask_fill(250, 300).unwrap();
        balance.release_ask(350).unwrap();

        assert_eq!(
            balance,
            MakerBalance {
                base_free: 750,
                base_locked: 0,
                quote_free: 350,
                quote_locked: 0,
            }
        );
    }

    #[test]
    fn bid_reservation_release_and_fill_conserve_assets() {
        let mut balance = MakerBalance {
            base_free: 10,
            base_locked: 0,
            quote_free: 1_000,
            quote_locked: 0,
        };

        balance.reserve_bid(700).unwrap();
        balance.settle_bid_fill(200, 300).unwrap();
        balance.release_bid(400).unwrap();

        assert_eq!(
            balance,
            MakerBalance {
                base_free: 210,
                base_locked: 0,
                quote_free: 700,
                quote_locked: 0,
            }
        );
    }

    #[test]
    fn active_buy_settlement_updates_multiple_makers_and_totals() {
        let fills = [fill(100, 100), fill(150, 165)];
        let owner_indexes = [1u16, 0u16];
        let mut balances = [
            MakerBalance {
                base_free: 0,
                base_locked: 150,
                quote_free: 10,
                quote_locked: 0,
            },
            MakerBalance {
                base_free: 0,
                base_locked: 100,
                quote_free: 20,
                quote_locked: 0,
            },
        ];

        let totals =
            settle_buy_active_fills(&fills, &owner_indexes, &mut balances).unwrap();

        assert_eq!(
            totals,
            ActiveSettlementTotals {
                base_to_taker: 250,
                quote_from_taker: 265,
            }
        );
        assert_eq!(balances[0].base_locked, 0);
        assert_eq!(balances[0].quote_free, 175);
        assert_eq!(balances[1].base_locked, 0);
        assert_eq!(balances[1].quote_free, 120);
    }

    #[test]
    fn settlement_rejects_uncollateralized_fill() {
        let fills = [fill(100, 100)];
        let owner_indexes = [0u16];
        let mut balances = [MakerBalance {
            base_free: 0,
            base_locked: 99,
            quote_free: 0,
            quote_locked: 0,
        }];

        assert_eq!(
            settle_buy_active_fills(&fills, &owner_indexes, &mut balances),
            Err(SettlementError::InsufficientBase)
        );
    }
}
