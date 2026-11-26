//! Versioned LP claims. Pure accounting only: callers must authenticate the
//! complete roster, account ownership and vaults before migrating or paying.
use crate::passive::{PassiveAccountingError as Error, PassivePool, PositionAccount};
use crate::passive_segments::{proportion, RangeClaim, SegmentedPool};
use hybrid_engine::range_position::RangePosition;

pub const POSITION_V2_BYTES: usize = 176;
pub const POSITION_V2_MAGIC: [u8; 8] = *b"HYBPOS02";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionV2 {
    pub bump: u8,
    pub market: [u8; 32],
    pub owner: [u8; 32],
    pub nonce: u64,
    pub claim: RangeClaim,
    pub revision: u64,
}

impl PositionV2 {
    pub fn validate(&self) -> Result<(), Error> {
        if self.market == [0; 32] || self.owner == [0; 32] || self.revision == 0 {
            return Err(Error::InvalidPosition);
        }
        self.claim.range.validate().map_err(|_| Error::InvalidPosition)?;
        self.claim.base.checked_add(self.claim.base_fees).ok_or(Error::Overflow)?;
        self.claim.quote.checked_add(self.claim.quote_fees).ok_or(Error::Overflow)?;
        Ok(())
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), Error> {
        self.validate()?;
        if out.len() != POSITION_V2_BYTES {
            return Err(Error::InvalidPosition);
        }
        out.fill(0);
        out[..8].copy_from_slice(&POSITION_V2_MAGIC);
        out[8] = 2;
        out[9] = self.bump;
        out[16..48].copy_from_slice(&self.market);
        out[48..80].copy_from_slice(&self.owner);
        out[80..88].copy_from_slice(&self.nonce.to_le_bytes());
        out[88..104].copy_from_slice(&self.claim.range.lower_sqrt_price_x64.to_le_bytes());
        out[104..120].copy_from_slice(&self.claim.range.upper_sqrt_price_x64.to_le_bytes());
        out[120..136].copy_from_slice(&self.claim.range.liquidity.to_le_bytes());
        for (i, value) in [
            self.claim.base,
            self.claim.quote,
            self.claim.base_fees,
            self.claim.quote_fees,
            self.revision,
        ]
        .iter()
        .enumerate()
        {
            out[136 + i * 8..144 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }

    pub fn decode_from(data: &[u8]) -> Result<Self, Error> {
        if data.len() != POSITION_V2_BYTES
            || data[..8] != POSITION_V2_MAGIC
            || data[8] != 2
            || data[10..16].iter().any(|b| *b != 0)
        {
            return Err(Error::InvalidPosition);
        }
        let u64_at = |i: usize| u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
        let u128_at = |i: usize| u128::from_le_bytes(data[i..i + 16].try_into().unwrap());
        let state = Self {
            bump: data[9],
            market: data[16..48].try_into().unwrap(),
            owner: data[48..80].try_into().unwrap(),
            nonce: u64_at(80),
            claim: RangeClaim {
                range: RangePosition {
                    lower_sqrt_price_x64: u128_at(88),
                    upper_sqrt_price_x64: u128_at(104),
                    liquidity: u128_at(120),
                },
                base: u64_at(136),
                quote: u64_at(144),
                base_fees: u64_at(152),
                quote_fees: u64_at(160),
            },
            revision: u64_at(168),
        };
        state.validate()?;
        Ok(state)
    }
}

/// Migrate a complete, authenticated legacy roster. Previously compounded
/// fees stay in inventory; separate historical fees may only be assigned to
/// the sole LP. No historical fee split is invented for multiple LPs.
pub fn migrate_roster(pool: PassivePool, legacy: &[PositionAccount]) -> Result<Vec<PositionV2>, Error> {
    if legacy.is_empty() || legacy.len() > 8 {
        return Err(Error::InvalidPosition);
    }
    if legacy.len() != 1 && (pool.accrued_base_fees != 0 || pool.accrued_quote_fees != 0) {
        return Err(Error::InvariantViolation);
    }
    let mut next = Vec::with_capacity(legacy.len());
    for (i, p) in legacy.iter().enumerate() {
        p.position.validate()?;
        if p.market != legacy[0].market
            || legacy[..i].iter().any(|q| q.position.owner == p.position.owner && q.nonce == p.nonce)
        {
            return Err(Error::InvalidPosition);
        }
        let v = PositionV2 {
            bump: p.bump,
            market: p.market,
            owner: p.position.owner,
            nonce: p.nonce,
            claim: RangeClaim {
                range: RangePosition {
                    lower_sqrt_price_x64: p.position.lower_sqrt_price_x64,
                    upper_sqrt_price_x64: p.position.upper_sqrt_price_x64,
                    liquidity: p.position.liquidity,
                },
                base: p.position.base_principal,
                quote: p.position.quote_principal,
                base_fees: if legacy.len() == 1 { pool.accrued_base_fees } else { 0 },
                quote_fees: if legacy.len() == 1 { pool.accrued_quote_fees } else { 0 },
            },
            revision: 1,
        };
        v.validate()?;
        next.push(v);
    }
    SegmentedPool { pool, positions: next.iter().map(|p| p.claim).collect() }.verify()?;
    Ok(next)
}

/// Partial burn leaves rounding dust with the remaining claim; full burn
/// returns every remaining atom. All changes are staged before commit.
pub fn redeem_liquidity(
    pool: &mut PassivePool,
    position: &mut PositionV2,
    expected_revision: u64,
    burn: u128,
) -> Result<(u64, u64), Error> {
    position.validate()?;
    let liquidity = position.claim.range.liquidity;
    if expected_revision != position.revision || burn == 0 || burn > liquidity {
        return Err(Error::InvalidPosition);
    }
    let mut p = *position;
    let mut next = *pool;
    let base = proportion(p.claim.base, burn, liquidity)?;
    let quote = proportion(p.claim.quote, burn, liquidity)?;
    let base_fee = proportion(p.claim.base_fees, burn, liquidity)?;
    let quote_fee = proportion(p.claim.quote_fees, burn, liquidity)?;
    let payout = (
        base.checked_add(base_fee).ok_or(Error::Overflow)?,
        quote.checked_add(quote_fee).ok_or(Error::Overflow)?,
    );
    if payout == (0, 0) {
        return Err(Error::ZeroAmount);
    }
    next.base_reserve = next.base_reserve.checked_sub(base).ok_or(Error::InsufficientReserves)?;
    next.quote_reserve = next.quote_reserve.checked_sub(quote).ok_or(Error::InsufficientReserves)?;
    next.accrued_base_fees = next.accrued_base_fees.checked_sub(base_fee).ok_or(Error::InsufficientReserves)?;
    next.accrued_quote_fees = next.accrued_quote_fees.checked_sub(quote_fee).ok_or(Error::InsufficientReserves)?;
    next.total_liquidity = next.total_liquidity.checked_sub(burn).ok_or(Error::InsufficientLiquidity)?;
    p.claim.base -= base;
    p.claim.quote -= quote;
    p.claim.base_fees -= base_fee;
    p.claim.quote_fees -= quote_fee;
    p.claim.range.liquidity -= burn;
    p.revision = p.revision.checked_add(1).ok_or(Error::Overflow)?;
    if burn != liquidity {
        p.validate()?;
    }
    // A fully redeemed position is a tombstone in memory; it cannot be
    // encoded as a live claim and the authenticated caller must close it.
    *pool = next;
    *position = p;
    Ok(payout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passive::{PassivePosition, PASSIVE_POSITION_BYTES};
    fn legacy(owner: u8) -> PositionAccount {
        PositionAccount {
            bump: 9,
            market: [1; 32],
            nonce: 1,
            position: PassivePosition {
                owner: [owner; 32],
                lower_sqrt_price_x64: 10,
                upper_sqrt_price_x64: 30,
                liquidity: 100,
                base_principal: 101,
                quote_principal: 201,
            },
        }
    }
    fn pool() -> PassivePool {
        PassivePool {
            base_reserve: 101,
            quote_reserve: 201,
            total_liquidity: 100,
            accrued_base_fees: 3,
            accrued_quote_fees: 5,
        }
    }
    #[test]
    fn codecs_do_not_alias_and_migration_preserves_legacy_claim() {
        let old = legacy(2);
        let mut v1 = [0; PASSIVE_POSITION_BYTES];
        old.encode_into(&mut v1).unwrap();
        let v = migrate_roster(pool(), &[old]).unwrap()[0];
        let mut encoded = [0; POSITION_V2_BYTES];
        v.encode_into(&mut encoded).unwrap();
        assert_eq!(PositionV2::decode_from(&encoded), Ok(v));
        assert!(PositionV2::decode_from(&v1).is_err());
        assert!(PositionAccount::decode_from(&encoded).is_err());
        encoded[10] = 1;
        assert!(PositionV2::decode_from(&encoded).is_err());
        assert_eq!(PositionAccount::decode_from(&v1), Ok(old));
    }
    #[test]
    fn ambiguous_or_incomplete_migration_fails() {
        assert!(migrate_roster(pool(), &[legacy(2), legacy(3)]).is_err());
        let mut p = pool();
        p.accrued_base_fees = 0;
        p.accrued_quote_fees = 0;
        assert!(migrate_roster(p, &[legacy(2), legacy(3)]).is_err());
        assert!(migrate_roster(p, &[legacy(2), legacy(2)]).is_err());
    }
    #[test]
    fn partial_then_full_redemption_conserves_every_atom_and_rejects_replay() {
        let mut pool = pool();
        let mut p = migrate_roster(pool, &[legacy(2)]).unwrap()[0];
        assert_eq!(redeem_liquidity(&mut pool, &mut p, 1, 33), Ok((33, 67)));
        let before = (pool, p);
        assert!(redeem_liquidity(&mut pool, &mut p, 1, 33).is_err());
        assert_eq!((pool, p), before);
        assert_eq!(redeem_liquidity(&mut pool, &mut p, 2, 67), Ok((71, 139)));
        assert_eq!(pool, PassivePool::default());
        assert!(p.validate().is_err());
    }
}
