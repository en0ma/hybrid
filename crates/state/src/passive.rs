//! Passive LP reserve segregation and checked accounting.
//!
//! This is an off-chain/on-chain pure state-transition foundation. It does not
//! authorize token transfers; the executable processor must validate vaults,
//! owner signatures, fee attribution, and position PDA ownership before use.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassiveAccountingError {
    ZeroAmount,
    Overflow,
    InsufficientReserves,
    InsufficientLiquidity,
    InvalidPosition,
    InvariantViolation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassivePool {
    /// Base atoms reserved for passive LPs, excluding active maker collateral.
    pub base_reserve: u64,
    /// Quote atoms reserved for passive LPs, excluding active maker collateral.
    pub quote_reserve: u64,
    pub total_liquidity: u128,
    pub accrued_base_fees: u64,
    pub accrued_quote_fees: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassivePosition {
    pub owner: [u8; 32],
    pub lower_sqrt_price_x64: u128,
    pub upper_sqrt_price_x64: u128,
    pub liquidity: u128,
    /// These amounts are explicitly reserved to this position, rather than
    /// inferred from a mutable global spot price.
    pub base_principal: u64,
    pub quote_principal: u64,
}

impl PassivePool {
    /// Add an explicitly collateralized position. All arithmetic is checked
    /// before any state is mutated, including the liquidity total.
    pub fn deposit(&mut self, position: &PassivePosition) -> Result<(), PassiveAccountingError> {
        position.validate()?;
        let base = self
            .base_reserve
            .checked_add(position.base_principal)
            .ok_or(PassiveAccountingError::Overflow)?;
        let quote = self
            .quote_reserve
            .checked_add(position.quote_principal)
            .ok_or(PassiveAccountingError::Overflow)?;
        let liquidity = self
            .total_liquidity
            .checked_add(position.liquidity)
            .ok_or(PassiveAccountingError::Overflow)?;
        self.base_reserve = base;
        self.quote_reserve = quote;
        self.total_liquidity = liquidity;
        Ok(())
    }

    /// Remove precisely this position's principal once all outstanding
    /// exposure has been settled by the surrounding swap/LP processor.
    pub fn withdraw(&mut self, position: &PassivePosition) -> Result<(), PassiveAccountingError> {
        position.validate()?;
        let base = self
            .base_reserve
            .checked_sub(position.base_principal)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        let quote = self
            .quote_reserve
            .checked_sub(position.quote_principal)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        let liquidity = self
            .total_liquidity
            .checked_sub(position.liquidity)
            .ok_or(PassiveAccountingError::InsufficientLiquidity)?;
        self.base_reserve = base;
        self.quote_reserve = quote;
        self.total_liquidity = liquidity;
        Ok(())
    }

    /// Apply a passive BUY (quote in, base out) with explicit fee exclusion.
    /// Fee amounts are separated from LP reserves, not silently added to
    /// active maker collateral or principal.
    pub fn buy(
        &mut self,
        quote_in: u64,
        base_out: u64,
        quote_fee: u64,
    ) -> Result<(), PassiveAccountingError> {
        if quote_in == 0 || base_out == 0 || quote_fee >= quote_in {
            return Err(PassiveAccountingError::ZeroAmount);
        }
        let base = self
            .base_reserve
            .checked_sub(base_out)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        let quote = self
            .quote_reserve
            .checked_add(quote_in - quote_fee)
            .ok_or(PassiveAccountingError::Overflow)?;
        let fees = self
            .accrued_quote_fees
            .checked_add(quote_fee)
            .ok_or(PassiveAccountingError::Overflow)?;
        self.base_reserve = base;
        self.quote_reserve = quote;
        self.accrued_quote_fees = fees;
        Ok(())
    }

    /// Apply a passive SELL (base in, quote out), fees taken in base.
    pub fn sell(
        &mut self,
        base_in: u64,
        quote_out: u64,
        base_fee: u64,
    ) -> Result<(), PassiveAccountingError> {
        if base_in == 0 || quote_out == 0 || base_fee >= base_in {
            return Err(PassiveAccountingError::ZeroAmount);
        }
        let base = self
            .base_reserve
            .checked_add(base_in - base_fee)
            .ok_or(PassiveAccountingError::Overflow)?;
        let quote = self
            .quote_reserve
            .checked_sub(quote_out)
            .ok_or(PassiveAccountingError::InsufficientReserves)?;
        let fees = self
            .accrued_base_fees
            .checked_add(base_fee)
            .ok_or(PassiveAccountingError::Overflow)?;
        self.base_reserve = base;
        self.quote_reserve = quote;
        self.accrued_base_fees = fees;
        Ok(())
    }

    /// Assert vaults cover active collateral, passive reserves and accrued
    /// fees without counting any token twice.
    pub fn verify_vault_coverage(
        &self,
        active_base: u64,
        active_quote: u64,
        actual_base: u64,
        actual_quote: u64,
    ) -> Result<(), PassiveAccountingError> {
        let required_base = active_base
            .checked_add(self.base_reserve)
            .and_then(|n| n.checked_add(self.accrued_base_fees))
            .ok_or(PassiveAccountingError::Overflow)?;
        let required_quote = active_quote
            .checked_add(self.quote_reserve)
            .and_then(|n| n.checked_add(self.accrued_quote_fees))
            .ok_or(PassiveAccountingError::Overflow)?;
        if actual_base < required_base || actual_quote < required_quote {
            return Err(PassiveAccountingError::InvariantViolation);
        }
        Ok(())
    }
}

impl PassivePosition {
    pub fn validate(&self) -> Result<(), PassiveAccountingError> {
        if self.owner == [0; 32]
            || self.lower_sqrt_price_x64 == 0
            || self.lower_sqrt_price_x64 >= self.upper_sqrt_price_x64
            || self.liquidity == 0
            || (self.base_principal == 0 && self.quote_principal == 0)
            // Until a range mint formula is verified, bound declared
            // liquidity by deposited principal. This is a safety cap,
            // NOT a liquidity calculation or permission to trade.
            || self.liquidity
                > u128::from(self.base_principal) + u128::from(self.quote_principal)
        {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        Ok(())
    }
}

/// Fixed-width account formats. No Rust memory layout is persisted.
pub const PASSIVE_POOL_BYTES: usize = 112;
pub const PASSIVE_POSITION_BYTES: usize = 160;
pub const PASSIVE_POOL_MAGIC: [u8; 8] = *b"HYBPOOL1";
pub const PASSIVE_POSITION_MAGIC: [u8; 8] = *b"HYBPOS01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolAccount {
    pub bump: u8,
    pub market: [u8; 32],
    pub pool: PassivePool,
}

impl PoolAccount {
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), PassiveAccountingError> {
        if out.len() != PASSIVE_POOL_BYTES || self.market == [0; 32] {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        out.fill(0);
        out[..8].copy_from_slice(&PASSIVE_POOL_MAGIC);
        out[8] = 1;
        out[9] = self.bump;
        out[16..48].copy_from_slice(&self.market);
        out[48..56].copy_from_slice(&self.pool.base_reserve.to_le_bytes());
        out[56..64].copy_from_slice(&self.pool.quote_reserve.to_le_bytes());
        out[64..80].copy_from_slice(&self.pool.total_liquidity.to_le_bytes());
        out[80..88].copy_from_slice(&self.pool.accrued_base_fees.to_le_bytes());
        out[88..96].copy_from_slice(&self.pool.accrued_quote_fees.to_le_bytes());
        Ok(())
    }

    pub fn decode_from(data: &[u8]) -> Result<Self, PassiveAccountingError> {
        if data.len() != PASSIVE_POOL_BYTES
            || data[..8] != PASSIVE_POOL_MAGIC
            || data[8] != 1
            || data[10..16].iter().any(|byte| *byte != 0)
            || data[96..].iter().any(|byte| *byte != 0)
        {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        let market: [u8; 32] = data[16..48]
            .try_into()
            .map_err(|_| PassiveAccountingError::InvalidPosition)?;
        if market == [0; 32] {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        Ok(Self {
            bump: data[9],
            market,
            pool: PassivePool {
                base_reserve: u64::from_le_bytes(
                    data[48..56]
                        .try_into()
                        .map_err(|_| PassiveAccountingError::InvalidPosition)?,
                ),
                quote_reserve: u64::from_le_bytes(
                    data[56..64]
                        .try_into()
                        .map_err(|_| PassiveAccountingError::InvalidPosition)?,
                ),
                total_liquidity: u128::from_le_bytes(
                    data[64..80]
                        .try_into()
                        .map_err(|_| PassiveAccountingError::InvalidPosition)?,
                ),
                accrued_base_fees: u64::from_le_bytes(
                    data[80..88]
                        .try_into()
                        .map_err(|_| PassiveAccountingError::InvalidPosition)?,
                ),
                accrued_quote_fees: u64::from_le_bytes(
                    data[88..96]
                        .try_into()
                        .map_err(|_| PassiveAccountingError::InvalidPosition)?,
                ),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionAccount {
    pub bump: u8,
    pub market: [u8; 32],
    pub nonce: u64,
    pub position: PassivePosition,
}

impl PositionAccount {
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), PassiveAccountingError> {
        self.position.validate()?;
        if out.len() != PASSIVE_POSITION_BYTES || self.market == [0; 32] {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        out.fill(0);
        out[..8].copy_from_slice(&PASSIVE_POSITION_MAGIC);
        out[8] = 1;
        out[9] = self.bump;
        out[16..48].copy_from_slice(&self.market);
        out[48..80].copy_from_slice(&self.position.owner);
        out[80..88].copy_from_slice(&self.nonce.to_le_bytes());
        out[88..104].copy_from_slice(&self.position.lower_sqrt_price_x64.to_le_bytes());
        out[104..120].copy_from_slice(&self.position.upper_sqrt_price_x64.to_le_bytes());
        out[120..136].copy_from_slice(&self.position.liquidity.to_le_bytes());
        out[136..144].copy_from_slice(&self.position.base_principal.to_le_bytes());
        out[144..152].copy_from_slice(&self.position.quote_principal.to_le_bytes());
        Ok(())
    }

    pub fn decode_from(data: &[u8]) -> Result<Self, PassiveAccountingError> {
        if data.len() != PASSIVE_POSITION_BYTES
            || data[..8] != PASSIVE_POSITION_MAGIC
            || data[8] != 1
            || data[10..16].iter().any(|byte| *byte != 0)
            || data[152..].iter().any(|byte| *byte != 0)
        {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        let bytes32 = |range: core::ops::Range<usize>| -> Result<[u8; 32], PassiveAccountingError> {
            data[range]
                .try_into()
                .map_err(|_| PassiveAccountingError::InvalidPosition)
        };
        let bytes16 = |range: core::ops::Range<usize>| -> Result<[u8; 16], PassiveAccountingError> {
            data[range]
                .try_into()
                .map_err(|_| PassiveAccountingError::InvalidPosition)
        };
        let bytes8 = |range: core::ops::Range<usize>| -> Result<[u8; 8], PassiveAccountingError> {
            data[range]
                .try_into()
                .map_err(|_| PassiveAccountingError::InvalidPosition)
        };
        let state = Self {
            bump: data[9],
            market: bytes32(16..48)?,
            nonce: u64::from_le_bytes(bytes8(80..88)?),
            position: PassivePosition {
                owner: bytes32(48..80)?,
                lower_sqrt_price_x64: u128::from_le_bytes(bytes16(88..104)?),
                upper_sqrt_price_x64: u128::from_le_bytes(bytes16(104..120)?),
                liquidity: u128::from_le_bytes(bytes16(120..136)?),
                base_principal: u64::from_le_bytes(bytes8(136..144)?),
                quote_principal: u64::from_le_bytes(bytes8(144..152)?),
            },
        };
        if state.market == [0; 32] {
            return Err(PassiveAccountingError::InvalidPosition);
        }
        state.position.validate()?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position() -> PassivePosition {
        PassivePosition {
            owner: [7; 32],
            lower_sqrt_price_x64: 10,
            upper_sqrt_price_x64: 30,
            liquidity: 100,
            base_principal: 200,
            quote_principal: 300,
        }
    }

    #[test]
    fn position_rejects_unbacked_liquidity_without_mutating_pool() {
        let mut pool = PassivePool::default();
        let mut bad = position();
        bad.liquidity = u128::MAX;
        bad.base_principal = 1;
        bad.quote_principal = 0;
        assert_eq!(bad.validate(), Err(PassiveAccountingError::InvalidPosition));
        assert_eq!(pool.deposit(&bad), Err(PassiveAccountingError::InvalidPosition));
        assert_eq!(pool, PassivePool::default());

        let mut small = position();
        small.base_principal = 1;
        small.quote_principal = 1;
        small.liquidity = 2;
        pool.deposit(&small).unwrap();
        assert_eq!(pool.total_liquidity, 2);
    }

    #[test]
    fn account_codecs_round_trip_and_reject_corruption() {
        let pool = PoolAccount {
            bump: 3,
            market: [1; 32],
            pool: PassivePool {
                base_reserve: 100,
                quote_reserve: 200,
                total_liquidity: 500,
                accrued_base_fees: 7,
                accrued_quote_fees: 9,
            },
        };
        let mut pool_bytes = [0; PASSIVE_POOL_BYTES];
        pool.encode_into(&mut pool_bytes).unwrap();
        assert_eq!(PoolAccount::decode_from(&pool_bytes), Ok(pool));
        pool_bytes[105] = 1;
        assert!(PoolAccount::decode_from(&pool_bytes).is_err());

        let position = PositionAccount {
            bump: 4,
            market: [1; 32],
            nonce: 42,
            position: position(),
        };
        let mut position_bytes = [0; PASSIVE_POSITION_BYTES];
        position.encode_into(&mut position_bytes).unwrap();
        assert_eq!(PositionAccount::decode_from(&position_bytes), Ok(position));
        position_bytes[152] = 1;
        assert!(PositionAccount::decode_from(&position_bytes).is_err());
        position_bytes[152] = 0;
        position_bytes[88..104].copy_from_slice(&30u128.to_le_bytes());
        assert!(PositionAccount::decode_from(&position_bytes).is_err());
    }

    #[test]
    fn position_deposit_withdraw_round_trip() {
        let mut pool = PassivePool::default();
        pool.deposit(&position()).unwrap();
        assert_eq!(
            (pool.base_reserve, pool.quote_reserve, pool.total_liquidity),
            (200, 300, 100)
        );
        pool.verify_vault_coverage(50, 70, 250, 370).unwrap();
        assert!(pool.verify_vault_coverage(50, 70, 249, 370).is_err());
        pool.withdraw(&position()).unwrap();
        assert_eq!(pool, PassivePool::default());
    }

    #[test]
    fn two_directions_preserve_gross_token_conservation() {
        let mut pool = PassivePool::default();
        pool.deposit(&position()).unwrap();
        pool.buy(25, 10, 2).unwrap();
        assert_eq!(
            (
                pool.base_reserve,
                pool.quote_reserve,
                pool.accrued_quote_fees
            ),
            (190, 323, 2)
        );
        pool.sell(20, 15, 3).unwrap();
        assert_eq!(
            (
                pool.base_reserve,
                pool.quote_reserve,
                pool.accrued_base_fees
            ),
            (207, 308, 3)
        );
        pool.verify_vault_coverage(50, 70, 260, 380).unwrap();
        assert!(pool.verify_vault_coverage(50, 70, 259, 380).is_err());
    }

    #[test]
    fn invalid_and_insolvent_transitions_do_not_mutate() {
        let mut pool = PassivePool::default();
        let before = pool;
        assert_eq!(
            pool.withdraw(&position()),
            Err(PassiveAccountingError::InsufficientReserves)
        );
        assert_eq!(pool, before);
        assert_eq!(pool.buy(10, 1, 10), Err(PassiveAccountingError::ZeroAmount));
        assert_eq!(pool, before);
        assert_eq!(
            pool.sell(10, 1, 0),
            Err(PassiveAccountingError::InsufficientReserves)
        );
        assert_eq!(pool, before);
    }

    #[test]
    fn overflow_and_invalid_bounds_are_rejected_atomically() {
        let mut pool = PassivePool {
            base_reserve: u64::MAX,
            ..PassivePool::default()
        };
        let before = pool;
        assert_eq!(
            pool.deposit(&position()),
            Err(PassiveAccountingError::Overflow)
        );
        assert_eq!(pool, before);
        let mut invalid = position();
        invalid.upper_sqrt_price_x64 = invalid.lower_sqrt_price_x64;
        assert_eq!(
            pool.deposit(&invalid),
            Err(PassiveAccountingError::InvalidPosition)
        );
        assert_eq!(pool, before);
    }

    #[test]
    fn full_fee_accrual_is_reserved_separately() {
        let mut pool = PassivePool::default();
        pool.deposit(&position()).unwrap();
        pool.buy(10, 1, 2).unwrap();
        assert_eq!(pool.verify_vault_coverage(0, 0, 199, 310), Ok(()));
        assert!(pool.verify_vault_coverage(0, 0, 199, 309).is_err());
    }
}
