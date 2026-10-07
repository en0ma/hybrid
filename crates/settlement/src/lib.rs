#![forbid(unsafe_code)]

use hybrid_engine::ActiveFill;

pub const MAKER_BALANCE_ACCOUNT_BYTES: usize = 64;

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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MakerBalanceAccount {
    pub owner: [u8; 32],
    pub balance: MakerBalance,
}

impl MakerBalanceAccount {
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), SettlementError> {
        if out.len() != MAKER_BALANCE_ACCOUNT_BYTES || self.owner == [0; 32] {
            return Err(SettlementError::InvalidFill);
        }
        out[0..32].copy_from_slice(&self.owner);
        out[32..40].copy_from_slice(&self.balance.base_free.to_le_bytes());
        out[40..48].copy_from_slice(&self.balance.base_locked.to_le_bytes());
        out[48..56].copy_from_slice(&self.balance.quote_free.to_le_bytes());
        out[56..64].copy_from_slice(&self.balance.quote_locked.to_le_bytes());
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, SettlementError> {
        if input.len() != MAKER_BALANCE_ACCOUNT_BYTES {
            return Err(SettlementError::InvalidFill);
        }
        let mut owner = [0u8; 32];
        owner.copy_from_slice(&input[0..32]);
        if owner == [0; 32] {
            return Err(SettlementError::InvalidFill);
        }
        Ok(Self {
            owner,
            balance: MakerBalance {
                base_free: u64::from_le_bytes(input[32..40].try_into().expect("base free")),
                base_locked: u64::from_le_bytes(input[40..48].try_into().expect("base locked")),
                quote_free: u64::from_le_bytes(input[48..56].try_into().expect("quote free")),
                quote_locked: u64::from_le_bytes(input[56..64].try_into().expect("quote locked")),
            },
        })
    }
}

const _: [(); MAKER_BALANCE_ACCOUNT_BYTES] = [(); core::mem::size_of::<MakerBalanceAccount>()];

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
    fn maker_balance_account_round_trips_and_has_exact_layout() {
        let account = MakerBalanceAccount {
            owner: [7; 32],
            balance: MakerBalance {
                base_free: 1,
                base_locked: 2,
                quote_free: 3,
                quote_locked: 4,
            },
        };
        let mut bytes = [0u8; MAKER_BALANCE_ACCOUNT_BYTES];
        account.encode_into(&mut bytes).unwrap();
        assert_eq!(MakerBalanceAccount::decode_from(&bytes).unwrap(), account);
        println!(
            "HYBRID_STATE_BYTES maker_balance_account {}",
            core::mem::size_of::<MakerBalanceAccount>()
        );
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
