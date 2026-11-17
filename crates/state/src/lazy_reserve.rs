//! Per-market, program-owned collateral segregation for pending lazy fills.
//! This account is the cross-epoch source of truth. No caller-supplied root
//! or Merkle proof may mint a liability. Only authenticated execution paths
//! may add reserves; only atomic claim+SPL paths may release them.
pub const LAZY_RESERVE_BYTES: usize = 144;
pub const LAZY_RESERVE_MAGIC: [u8; 8] = *b"HYBLAZY1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LazyReserveError {
    Invalid,
    Overflow,
    Underflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LazyReserve {
    pub bump: u8,
    pub market: [u8; 32],
    /// Deployment-authorized cluster-domain identity.
    pub genesis: [u8; 32],
    /// Next immutable epoch identifier. No reusing a previously settled ID.
    pub next_epoch_id: u64,
    /// Pending maker credits are NOT counted in CustodyState.total_*.
    pub maker_base_credit: u64,
    pub maker_quote_credit: u64,
    /// Pending maker debits have ALREADY left vault/custody at execution.
    /// The corresponding maker's locked balance is cleared only on claim.
    pub maker_base_debit: u64,
    pub maker_quote_debit: u64,
    /// Program fees stay segregated until a separately reviewed fee sweep.
    pub base_fees: u64,
    pub quote_fees: u64,
}

impl LazyReserve {
    pub fn new(bump: u8, market: [u8; 32], genesis: [u8; 32]) -> Result<Self, LazyReserveError> {
        let state = Self {
            bump,
            market,
            genesis,
            next_epoch_id: 0,
            maker_base_credit: 0,
            maker_quote_credit: 0,
            maker_base_debit: 0,
            maker_quote_debit: 0,
            base_fees: 0,
            quote_fees: 0,
        };
        state.validate()?;
        Ok(state)
    }
    fn validate(&self) -> Result<(), LazyReserveError> {
        if self.market == [0; 32] || self.genesis == [0; 32] {
            return Err(LazyReserveError::Invalid);
        }
        Ok(())
    }
    pub fn encode(&self, data: &mut [u8]) -> Result<(), LazyReserveError> {
        self.validate()?;
        if data.len() != LAZY_RESERVE_BYTES {
            return Err(LazyReserveError::Invalid);
        }
        data.fill(0);
        data[0..8].copy_from_slice(&LAZY_RESERVE_MAGIC);
        data[8] = 1;
        data[9] = self.bump;
        data[16..48].copy_from_slice(&self.market);
        data[48..80].copy_from_slice(&self.genesis);
        for (i, field) in [
            self.next_epoch_id,
            self.maker_base_credit,
            self.maker_quote_credit,
            self.maker_base_debit,
            self.maker_quote_debit,
            self.base_fees,
            self.quote_fees,
        ]
        .iter()
        .enumerate()
        {
            let offset = 80 + i * 8;
            data[offset..offset + 8].copy_from_slice(&field.to_le_bytes());
        }
        Ok(())
    }
    pub fn decode(data: &[u8]) -> Result<Self, LazyReserveError> {
        if data.len() != LAZY_RESERVE_BYTES
            || data[0..8] != LAZY_RESERVE_MAGIC
            || data[8] != 1
            || data[10..16] != [0; 6]
            || data[136..144] != [0; 8]
        {
            return Err(LazyReserveError::Invalid);
        }
        let get = |i: usize| -> u64 {
            let start = 80 + i * 8;
            u64::from_le_bytes(data[start..start + 8].try_into().expect("fixed reserve"))
        };
        let state = Self {
            bump: data[9],
            market: data[16..48]
                .try_into()
                .map_err(|_| LazyReserveError::Invalid)?,
            genesis: data[48..80]
                .try_into()
                .map_err(|_| LazyReserveError::Invalid)?,
            next_epoch_id: get(0),
            maker_base_credit: get(1),
            maker_quote_credit: get(2),
            maker_base_debit: get(3),
            maker_quote_debit: get(4),
            base_fees: get(5),
            quote_fees: get(6),
        };
        state.validate()?;
        Ok(state)
    }

    /// Called only by a verified execution+sealing instruction. Precalculates
    /// every balance before updating, so an overflow never partially mutates.
    pub fn reserve_execution(
        &mut self,
        epoch_id: u64,
        totals: [u64; 6],
    ) -> Result<(), LazyReserveError> {
        if epoch_id != self.next_epoch_id {
            return Err(LazyReserveError::Invalid);
        }
        let [base_debit, base_credit, quote_debit, quote_credit, base_fee, quote_fee] = totals;
        let next = Self {
            next_epoch_id: self
                .next_epoch_id
                .checked_add(1)
                .ok_or(LazyReserveError::Overflow)?,
            maker_base_credit: self
                .maker_base_credit
                .checked_add(base_credit)
                .ok_or(LazyReserveError::Overflow)?,
            maker_quote_credit: self
                .maker_quote_credit
                .checked_add(quote_credit)
                .ok_or(LazyReserveError::Overflow)?,
            maker_base_debit: self
                .maker_base_debit
                .checked_add(base_debit)
                .ok_or(LazyReserveError::Overflow)?,
            maker_quote_debit: self
                .maker_quote_debit
                .checked_add(quote_debit)
                .ok_or(LazyReserveError::Overflow)?,
            base_fees: self
                .base_fees
                .checked_add(base_fee)
                .ok_or(LazyReserveError::Overflow)?,
            quote_fees: self
                .quote_fees
                .checked_add(quote_fee)
                .ok_or(LazyReserveError::Overflow)?,
            ..*self
        };
        *self = next;
        Ok(())
    }

    /// Called only after a claim has passed signer, epoch/root and vault
    /// checks. Must be paired atomically with epoch bitmap and SPL transfer.
    pub fn release_claim(&mut self, totals: [u64; 6]) -> Result<(), LazyReserveError> {
        let [base_debit, base_credit, quote_debit, quote_credit, _, _] = totals;
        let next = Self {
            maker_base_credit: self
                .maker_base_credit
                .checked_sub(base_credit)
                .ok_or(LazyReserveError::Underflow)?,
            maker_quote_credit: self
                .maker_quote_credit
                .checked_sub(quote_credit)
                .ok_or(LazyReserveError::Underflow)?,
            maker_base_debit: self
                .maker_base_debit
                .checked_sub(base_debit)
                .ok_or(LazyReserveError::Underflow)?,
            maker_quote_debit: self
                .maker_quote_debit
                .checked_sub(quote_debit)
                .ok_or(LazyReserveError::Underflow)?,
            ..*self
        };
        *self = next;
        Ok(())
    }
    /// Total on-chain vault quantity that cannot be spent by ordinary
    /// withdrawals, including existing normal custody balances and all
    /// unclaimed epoch credits and accrued protocol fees.
    pub fn vault_floor(
        &self,
        custody_base: u64,
        custody_quote: u64,
    ) -> Result<(u64, u64), LazyReserveError> {
        let base = custody_base
            .checked_add(self.maker_base_credit)
            .and_then(|value| value.checked_add(self.base_fees))
            .ok_or(LazyReserveError::Overflow)?;
        let quote = custody_quote
            .checked_add(self.maker_quote_credit)
            .and_then(|value| value.checked_add(self.quote_fees))
            .ok_or(LazyReserveError::Overflow)?;
        Ok((base, quote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_claim_and_vault_floors_conserve_both_assets() {
        let mut reserve = LazyReserve::new(2, [1; 32], [3; 32]).unwrap();
        reserve.reserve_execution(0, [3, 0, 0, 12, 0, 1]).unwrap();
        reserve.reserve_execution(1, [0, 4, 15, 0, 1, 0]).unwrap();
        assert_eq!(reserve.vault_floor(20, 30), Ok((25, 43)));
        reserve.release_claim([2, 0, 0, 7, 0, 1]).unwrap();
        assert_eq!(reserve.vault_floor(20, 30), Ok((25, 36)));
        reserve.release_claim([0, 4, 15, 0, 1, 0]).unwrap();
        reserve.release_claim([1, 0, 0, 5, 0, 0]).unwrap();
        assert_eq!(reserve.vault_floor(20, 30), Ok((21, 31)));
        assert_eq!(reserve.maker_base_debit, 0);
        assert_eq!(reserve.maker_quote_debit, 0);
        let mut bytes = [0; LAZY_RESERVE_BYTES];
        reserve.encode(&mut bytes).unwrap();
        assert_eq!(LazyReserve::decode(&bytes), Ok(reserve));
        bytes[10] = 1;
        assert_eq!(LazyReserve::decode(&bytes), Err(LazyReserveError::Invalid));
    }

    #[test]
    fn replay_underflow_overflow_and_epoch_reuse_preserve_state() {
        let mut reserve = LazyReserve::new(2, [1; 32], [3; 32]).unwrap();
        assert_eq!(
            reserve.reserve_execution(1, [1, 0, 0, 2, 0, 0]),
            Err(LazyReserveError::Invalid)
        );
        reserve.reserve_execution(0, [1, 0, 0, 2, 0, 0]).unwrap();
        let before = reserve;
        assert_eq!(
            reserve.reserve_execution(0, [1, 0, 0, 2, 0, 0]),
            Err(LazyReserveError::Invalid)
        );
        assert_eq!(
            reserve.release_claim([2, 0, 0, 2, 0, 0]),
            Err(LazyReserveError::Underflow)
        );
        assert_eq!(reserve, before);
        reserve.maker_quote_credit = u64::MAX;
        let before_overflow = reserve;
        assert_eq!(
            reserve.reserve_execution(1, [1, 0, 0, 1, 0, 0]),
            Err(LazyReserveError::Overflow)
        );
        assert_eq!(reserve, before_overflow);
    }
}
