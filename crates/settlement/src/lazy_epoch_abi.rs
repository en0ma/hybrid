//! Canonical binary frozen epoch v2 commitment (read-only).
//! No Solana owner checks, on-chain sealing, custody or payout authorization.
//! This is the exact Rust peer of sdk/src/pro-rata-epoch-abi.mjs.
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"HYBRID_LAZY_EPOCH_V2\0";
pub const MAX_ROSTER: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochAbiError {
    InvalidRoster,
    InvalidMaker,
    InvalidWeight,
    InvalidFill,
    InvalidDirection,
    InvalidTick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Ask,
    Bid,
}

pub struct EpochMaker<'a> {
    pub id: &'a str,
    pub weight: u64,
}

#[derive(Clone, Copy)]
pub struct FrozenEpochV2<'a> {
    pub market: [u8; 32],
    pub program: [u8; 32],
    pub genesis: [u8; 32],
    pub epoch_id: u64,
    pub tick_sqrt_price_x64: u128,
    pub side: Side,
    /// Exact frozen roster / dust-priority order.
    pub makers: &'a [EpochMaker<'a>],
    /// baseDebit, baseCredit, quoteDebit, quoteCredit, baseFee, quoteFee.
    pub totals: [u64; 6],
}

pub fn hash_frozen_epoch_v2(epoch: &FrozenEpochV2<'_>) -> Result<[u8; 32], EpochAbiError> {
    if epoch.makers.is_empty() || epoch.makers.len() > MAX_ROSTER {
        return Err(EpochAbiError::InvalidRoster);
    }
    if epoch.tick_sqrt_price_x64 == 0 {
        return Err(EpochAbiError::InvalidTick);
    }
    let mut weight = 0u128;
    for (index, maker) in epoch.makers.iter().enumerate() {
        let name = maker.id.as_bytes();
        if name.is_empty()
            || name.len() > 128
            || !name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        {
            return Err(EpochAbiError::InvalidMaker);
        }
        if epoch.makers[..index]
            .iter()
            .any(|prior| prior.id == maker.id)
        {
            return Err(EpochAbiError::InvalidMaker);
        }
        if maker.weight == 0 {
            return Err(EpochAbiError::InvalidWeight);
        }
        weight += u128::from(maker.weight);
    }
    let filled = match epoch.side {
        Side::Ask => {
            if epoch.totals[1] != 0 || epoch.totals[2] != 0 || epoch.totals[4] != 0 {
                return Err(EpochAbiError::InvalidDirection);
            }
            epoch.totals[0]
        }
        Side::Bid => {
            if epoch.totals[0] != 0 || epoch.totals[3] != 0 || epoch.totals[5] != 0 {
                return Err(EpochAbiError::InvalidDirection);
            }
            epoch.totals[1]
        }
    };
    if filled == 0 || u128::from(filled) > weight {
        return Err(EpochAbiError::InvalidFill);
    }

    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(epoch.market);
    hash.update(epoch.program);
    hash.update(epoch.genesis);
    hash.update(epoch.epoch_id.to_le_bytes());
    hash.update(epoch.tick_sqrt_price_x64.to_le_bytes());
    hash.update([match epoch.side {
        Side::Ask => 0,
        Side::Bid => 1,
    }]);
    hash.update((epoch.makers.len() as u32).to_le_bytes());
    for (index, maker) in epoch.makers.iter().enumerate() {
        hash.update((index as u32).to_le_bytes());
        let name = maker.id.as_bytes();
        hash.update([name.len() as u8]);
        hash.update(name);
        hash.update(maker.weight.to_le_bytes());
    }
    for amount in epoch.totals {
        hash.update(amount.to_le_bytes());
    }
    Ok(hash.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn canonical_v2_epoch_digest_matches_sdk() {
        let makers = [
            EpochMaker { id: "A", weight: 1 },
            EpochMaker { id: "B", weight: 2 },
            EpochMaker { id: "C", weight: 3 },
        ];
        let epoch = FrozenEpochV2 {
            market: [0x11; 32],
            program: [0x22; 32],
            genesis: [0x33; 32],
            epoch_id: 7,
            tick_sqrt_price_x64: 1u128 << 64,
            side: Side::Ask,
            makers: &makers,
            totals: [2, 0, 0, 7, 0, 1],
        };
        assert_eq!(
            hex(&hash_frozen_epoch_v2(&epoch).unwrap()),
            "fddeaffac0081511043c404a4fca35e3363bc97e8d235c697aa682e239cfb00b"
        );
    }

    #[test]
    fn changed_market_tick_roster_or_fee_changes_digest() {
        let makers = [
            EpochMaker { id: "A", weight: 1 },
            EpochMaker { id: "B", weight: 2 },
        ];
        let original = FrozenEpochV2 {
            market: [0x11; 32],
            epoch_id: 7,
            tick_sqrt_price_x64: 1u128 << 64,
            side: Side::Ask,
            makers: &makers,
            totals: [1, 0, 0, 3, 0, 1],
        };
        let first = hash_frozen_epoch_v2(&original).unwrap();
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                market: [0x22; 32],
                ..original
            })
            .unwrap(),
            first
        );
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                program: [0x44; 32],
                ..original
            })
            .unwrap(),
            first
        );
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                genesis: [0x44; 32],
                ..original
            })
            .unwrap(),
            first
        );
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                tick_sqrt_price_x64: 2u128 << 64,
                ..original
            })
            .unwrap(),
            first
        );
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                totals: [1, 0, 0, 3, 0, 2],
                ..original
            })
            .unwrap(),
            first
        );
        let reordered = [
            EpochMaker { id: "B", weight: 2 },
            EpochMaker { id: "A", weight: 1 },
        ];
        assert_ne!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                makers: &reordered,
                ..original
            })
            .unwrap(),
            first
        );
    }

    #[test]
    fn reject_zero_weight_duplicate_maker_and_incoherent_base_fill() {
        let zero = [EpochMaker { id: "A", weight: 0 }];
        let duplicate = [
            EpochMaker { id: "A", weight: 1 },
            EpochMaker { id: "A", weight: 1 },
        ];
        let valid = [EpochMaker { id: "A", weight: 1 }];
        let base = FrozenEpochV2 {
            market: [0x11; 32],
            epoch_id: 7,
            tick_sqrt_price_x64: 1u128 << 64,
            side: Side::Ask,
            makers: &valid,
            totals: [1, 0, 0, 2, 0, 0],
        };
        assert_eq!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                makers: &zero,
                ..base
            }),
            Err(EpochAbiError::InvalidWeight)
        );
        assert_eq!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                makers: &duplicate,
                ..base
            }),
            Err(EpochAbiError::InvalidMaker)
        );
        assert_eq!(
            hash_frozen_epoch_v2(&FrozenEpochV2 {
                totals: [2, 0, 0, 2, 0, 0],
                ..base
            }),
            Err(EpochAbiError::InvalidFill)
        );
    }
}
