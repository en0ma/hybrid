//! On-chain-compatible fixed binary frozen-epoch storage.
//!
//! This module is a state primitive, not an executable sealing/withdrawal
//! instruction. A future processor MUST authenticate program ownership,
//! market PDA, signer/seat, epoch derivation, custody and token CPI before
//! using any state transition. Solana's transaction rollback must encompass
//! both claimed-bit changes and SPL transfers.
pub const FROZEN_EPOCH_BYTES: usize = 864;
pub const MAX_FROZEN_MAKERS: u32 = 4096;
pub const FROZEN_EPOCH_MAGIC: [u8; 8] = *b"HYBEPOCH";
pub const FROZEN_EPOCH_VERSION: u8 = 1;
pub const FROZEN_STATUS_SEALED: u8 = 1;
const CLAIM_BITMAP_OFFSET: usize = 352;
const CLAIM_BITMAP_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenEpochError {
    BufferSize,
    InvalidState,
    InvalidSeat,
    AlreadyClaimed,
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenEpochHeader {
    pub bump: u8,
    /// 0 = ask, 1 = bid.
    pub side: u8,
    pub market: [u8; 32],
    /// Protocol-authorized epoch sealer, not proof of having sealed correctly.
    pub authority: [u8; 32],
    pub program: [u8; 32],
    pub genesis: [u8; 32],
    pub epoch_id: u64,
    pub tick_sqrt_price_x64: u128,
    pub commitment: [u8; 32],
    pub claim_root: [u8; 32],
    pub maker_count: u32,
    /// baseDebit, baseCredit, quoteDebit, quoteCredit, baseFee, quoteFee.
    pub totals: [u64; 6],
    pub base_vault: [u8; 32],
    pub quote_vault: [u8; 32],
}

impl FrozenEpochHeader {
    pub fn validate(&self) -> Result<(), FrozenEpochError> {
        if self.side > 1
            || self.maker_count == 0
            || self.maker_count > MAX_FROZEN_MAKERS
            || self.tick_sqrt_price_x64 == 0
            || [
                self.market, self.authority, self.program, self.genesis,
                self.commitment, self.claim_root, self.base_vault, self.quote_vault,
            ].contains(&[0; 32])
            || self.base_vault == self.quote_vault
        {
            return Err(FrozenEpochError::InvalidState);
        }
        let [base_debit, base_credit, quote_debit, quote_credit, base_fee, quote_fee] =
            self.totals;
        if (self.side == 0
            && (base_debit == 0 || base_credit != 0 || quote_debit != 0 || base_fee != 0))
            || (self.side == 1
                && (base_credit == 0 || base_debit != 0 || quote_credit != 0 || quote_fee != 0))
        {
            return Err(FrozenEpochError::InvalidState);
        }
        Ok(())
    }

    /// Initialize a *new* zero-filled, rent-exempt account supplied by the
    /// caller. Ownership/PDA/sealer authorization must be checked upstream.
    pub fn initialize(&self, data: &mut [u8]) -> Result<(), FrozenEpochError> {
        if data.len() != FROZEN_EPOCH_BYTES {
            return Err(FrozenEpochError::BufferSize);
        }
        self.validate()?;
        if data.iter().any(|byte| *byte != 0) {
            return Err(FrozenEpochError::InvalidState);
        }
        data.fill(0);
        data[0..8].copy_from_slice(&FROZEN_EPOCH_MAGIC);
        data[8] = FROZEN_EPOCH_VERSION;
        data[9] = self.bump;
        data[10] = FROZEN_STATUS_SEALED;
        data[11] = self.side;
        data[16..48].copy_from_slice(&self.market);
        data[48..80].copy_from_slice(&self.authority);
        data[80..112].copy_from_slice(&self.program);
        data[112..144].copy_from_slice(&self.genesis);
        data[144..152].copy_from_slice(&self.epoch_id.to_le_bytes());
        data[152..168].copy_from_slice(&self.tick_sqrt_price_x64.to_le_bytes());
        data[168..200].copy_from_slice(&self.commitment);
        data[200..232].copy_from_slice(&self.claim_root);
        data[232..236].copy_from_slice(&self.maker_count.to_le_bytes());
        for (i, total) in self.totals.iter().enumerate() {
            data[236 + 8 * i..244 + 8 * i].copy_from_slice(&total.to_le_bytes());
        }
        data[284..316].copy_from_slice(&self.base_vault);
        data[316..348].copy_from_slice(&self.quote_vault);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenEpochView {
    pub header: FrozenEpochHeader,
    pub claimed_count: u32,
}

impl FrozenEpochView {
    /// Reject corrupt reserved bytes, uninitialized storage, claims outside
    /// the immutable maker roster and bitmap/count mismatches.
    pub fn decode(data: &[u8]) -> Result<Self, FrozenEpochError> {
        if data.len() != FROZEN_EPOCH_BYTES {
            return Err(FrozenEpochError::BufferSize);
        }
        if data[0..8] != FROZEN_EPOCH_MAGIC
            || data[8] != FROZEN_EPOCH_VERSION
            || data[10] != FROZEN_STATUS_SEALED
            || data[12..16] != [0; 4]
            || data[CLAIM_BITMAP_OFFSET..].len() != CLAIM_BITMAP_BYTES
        {
            return Err(FrozenEpochError::InvalidState);
        }
        let header = FrozenEpochHeader {
            bump: data[9],
            side: data[11],
            market: data[16..48].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            authority: data[48..80].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            program: data[80..112].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            genesis: data[112..144].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            epoch_id: u64::from_le_bytes(data[144..152].try_into().map_err(|_| FrozenEpochError::InvalidState)?),
            tick_sqrt_price_x64: u128::from_le_bytes(data[152..168].try_into().map_err(|_| FrozenEpochError::InvalidState)?),
            commitment: data[168..200].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            claim_root: data[200..232].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            maker_count: u32::from_le_bytes(data[232..236].try_into().map_err(|_| FrozenEpochError::InvalidState)?),
            totals: core::array::from_fn(|i| u64::from_le_bytes(
                data[236 + 8 * i..244 + 8 * i].try_into().expect("fixed length"),
            )),
            base_vault: data[284..316].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
            quote_vault: data[316..348].try_into().map_err(|_| FrozenEpochError::InvalidState)?,
        };
        header.validate()?;
        let claimed_count = u32::from_le_bytes(data[348..352].try_into().map_err(|_| FrozenEpochError::InvalidState)?);
        if claimed_count > header.maker_count {
            return Err(FrozenEpochError::InvalidState);
        }
        let mut popcount = 0u32;
        for seat in 0..MAX_FROZEN_MAKERS {
            let offset = CLAIM_BITMAP_OFFSET + (seat / 8) as usize;
            let mask = 1u8 << (seat % 8);
            if data[offset] & mask != 0 {
                if seat >= header.maker_count {
                    return Err(FrozenEpochError::InvalidState);
                }
                popcount += 1;
            }
        }
        if popcount != claimed_count {
            return Err(FrozenEpochError::InvalidState);
        }
        Ok(Self { header, claimed_count })
    }

    pub fn is_claimed(self, data: &[u8], seat: u32) -> Result<bool, FrozenEpochError> {
        if seat >= self.header.maker_count || data.len() != FROZEN_EPOCH_BYTES {
            return Err(FrozenEpochError::InvalidSeat);
        }
        let byte = CLAIM_BITMAP_OFFSET + (seat / 8) as usize;
        Ok(data[byte] & (1u8 << (seat % 8)) != 0)
    }

    /// Only call once signer, Merkle inclusion and custody invariants have
    /// been independently verified. Does NOT transfer or authorize tokens.
    /// This state update and the token CPI must be in the SAME instruction.
    pub fn mark_claimed(self, data: &mut [u8], seat: u32) -> Result<(), FrozenEpochError> {
        if seat >= self.header.maker_count {
            return Err(FrozenEpochError::InvalidSeat);
        }
        // Caller must not trust a stale decoded view after mutable storage
        // changes. Re-decode the live account to reject duplicate execution.
        let current = Self::decode(data)?;
        if current.header != self.header {
            return Err(FrozenEpochError::InvalidState);
        }
        if current.is_claimed(data, seat)? {
            return Err(FrozenEpochError::AlreadyClaimed);
        }
        let next = current.claimed_count.checked_add(1).ok_or(FrozenEpochError::Overflow)?;
        let byte = CLAIM_BITMAP_OFFSET + (seat / 8) as usize;
        data[byte] |= 1u8 << (seat % 8);
        data[348..352].copy_from_slice(&next.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(count: u32) -> FrozenEpochHeader {
        FrozenEpochHeader {
            bump: 42, side: 0, market: [1; 32], authority: [2; 32],
            program: [3; 32], genesis: [4; 32], epoch_id: 17,
            tick_sqrt_price_x64: 1u128 << 64, commitment: [5; 32],
            claim_root: [6; 32], maker_count: count,
            totals: [100, 0, 0, 250, 0, 3],
            base_vault: [7; 32], quote_vault: [8; 32],
        }
    }
    #[test]
    fn frozen_header_roundtrip_and_single_claim() {
        let mut account = [0u8; FROZEN_EPOCH_BYTES];
        let expected = header(3);
        expected.initialize(&mut account).unwrap();
        let view = FrozenEpochView::decode(&account).unwrap();
        assert_eq!(view.header, expected);
        assert_eq!(view.claimed_count, 0);
        assert_eq!(view.is_claimed(&account, 2), Ok(false));
        view.mark_claimed(&mut account, 2).unwrap();
        assert_eq!(FrozenEpochView::decode(&account).unwrap().claimed_count, 1);
        assert_eq!(view.is_claimed(&account, 2), Ok(true));
        assert_eq!(view.mark_claimed(&mut account, 2), Err(FrozenEpochError::AlreadyClaimed));
        assert_eq!(view.mark_claimed(&mut account, 3), Err(FrozenEpochError::InvalidSeat));
    }
    #[test]
    fn 4096_bitmap_boundary_and_out_of_roster_poison() {
        let mut account = [0u8; FROZEN_EPOCH_BYTES];
        header(MAX_FROZEN_MAKERS).initialize(&mut account).unwrap();
        let view = FrozenEpochView::decode(&account).unwrap();
        view.mark_claimed(&mut account, 4095).unwrap();
        assert_eq!(FrozenEpochView::decode(&account).unwrap().claimed_count, 1);
        assert_eq!(view.mark_claimed(&mut account, 4096), Err(FrozenEpochError::InvalidSeat));
        let mut smaller = [0u8; FROZEN_EPOCH_BYTES];
        header(3).initialize(&mut smaller).unwrap();
        smaller[CLAIM_BITMAP_OFFSET] = 1u8 << 7;
        smaller[348..352].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(FrozenEpochView::decode(&smaller), Err(FrozenEpochError::InvalidState));
    }
    #[test]
    fn corrupt_state_reserved_bytes_and_replayed_snapshot_fail() {
        let mut account = [0u8; FROZEN_EPOCH_BYTES];
        header(3).initialize(&mut account).unwrap();
        let initial = FrozenEpochView::decode(&account).unwrap();
        assert_eq!(header(3).initialize(&mut account), Err(FrozenEpochError::InvalidState));
        initial.mark_claimed(&mut account, 0).unwrap();
        // A previously decoded view can only update the current, validated
        // account, never a stale snapshot of a different header.
        account[168] ^= 1;
        assert_eq!(initial.mark_claimed(&mut account, 1), Err(FrozenEpochError::InvalidState));
        account[168] ^= 1;
        account[12] = 1;
        assert_eq!(FrozenEpochView::decode(&account), Err(FrozenEpochError::InvalidState));
    }
    #[test]
    fn malformed_totals_or_zero_authority_are_invalid() {
        let mut h = header(1);
        h.authority = [0; 32];
        assert_eq!(h.validate(), Err(FrozenEpochError::InvalidState));
        h = header(1);
        h.totals[2] = 1;
        assert_eq!(h.validate(), Err(FrozenEpochError::InvalidState));
    }
}
