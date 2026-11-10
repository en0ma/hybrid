//! Read-only signer-bound, pubkey-based frozen maker claim gate.
//!
//! This is a pure precondition check, NOT a payout or authority to move funds.
//! The caller (future Solana processor) MUST establish real AccountInfo owner,
//! signer, market PDA and vault identities before calling this function.
//! A trusted sealer must derive the immutable epoch root from verified trades.
//! No bitmap update and no SPL transfer is performed here.
use hybrid_state::frozen_epoch::FrozenEpochView;
use hybrid_state::MakerBalance;
use sha2::{Digest, Sha256};

const LEAF_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_CLAIM_V1\0";
const NODE_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_NODE_V1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenClaimError {
    InvalidEpoch,
    WrongProgram,
    WrongMarket,
    WrongCluster,
    UnauthorizedMaker,
    InvalidSeat,
    AlreadyClaimed,
    InvalidAmount,
    InvalidProof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PubkeyClaimLeaf {
    pub seat: u32,
    /// Actual maker signing authority, not an untrusted text identifier.
    pub maker: [u8; 32],
    pub weight: u64,
    /// baseDebit, baseCredit, quoteDebit, quoteCredit, baseFee, quoteFee.
    pub amounts: [u64; 6],
}

pub struct ClaimContext<'a> {
    /// Must come from the actual executing program and authenticated accounts.
    pub program: [u8; 32],
    pub market: [u8; 32],
    pub genesis: [u8; 32],
    pub signer: [u8; 32],
    /// Must come from AccountInfo::is_signer, not instruction data.
    pub is_signer: bool,
    /// Must have been decoded from the program-owned maker-balance account,
    /// whose PDA must be checked against the market and maker signer.
    pub maker_balance: &'a MakerBalance,
    /// Must have been read from the program-owned canonical epoch PDA.
    pub epoch_data: &'a [u8],
}

/// The preimage is domain | epoch commitment[32] | seat u32LE | maker
/// Pubkey[32] | weight u64LE | six amounts u64LE. Internal Merkle nodes
/// use a separate domain | left hash[32] | right hash[32].
pub fn pubkey_claim_leaf_hash(
    epoch_commitment: &[u8; 32],
    leaf: &PubkeyClaimLeaf,
) -> Result<[u8; 32], FrozenClaimError> {
    if leaf.seat >= 4096 || leaf.maker == [0; 32] || leaf.weight == 0 {
        return Err(FrozenClaimError::InvalidSeat);
    }
    let mut digest = Sha256::new();
    digest.update(LEAF_DOMAIN);
    digest.update(epoch_commitment);
    digest.update(leaf.seat.to_le_bytes());
    digest.update(leaf.maker);
    digest.update(leaf.weight.to_le_bytes());
    for amount in leaf.amounts {
        digest.update(amount.to_le_bytes());
    }
    Ok(digest.finalize().into())
}

pub fn pubkey_claim_node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(NODE_DOMAIN);
    digest.update(left);
    digest.update(right);
    digest.finalize().into()
}

/// Validate the frozen-epoch claim against the exact program/cluster/market,
/// program-owned maker identity (validated by caller), actual signer flag,
/// immutable claim root, directional atom totals, and live unclaimed bitmap.
/// Returns the approved seat for a future atomic transfer + bitmap mutation.
/// It does not authorize a transfer by itself.
pub fn verify_frozen_claim(
    context: &ClaimContext<'_>,
    leaf: &PubkeyClaimLeaf,
    siblings: &[[u8; 32]],
) -> Result<u32, FrozenClaimError> {
    let view =
        FrozenEpochView::decode(context.epoch_data).map_err(|_| FrozenClaimError::InvalidEpoch)?;
    let epoch = view.header;
    if epoch.program != context.program {
        return Err(FrozenClaimError::WrongProgram);
    }
    if epoch.market != context.market {
        return Err(FrozenClaimError::WrongMarket);
    }
    if epoch.genesis != context.genesis {
        return Err(FrozenClaimError::WrongCluster);
    }
    if !context.is_signer
        || leaf.maker != context.signer
        || context.maker_balance.owner != context.signer
        || context.maker_balance.market != context.market
        || leaf.maker == [0; 32]
    {
        return Err(FrozenClaimError::UnauthorizedMaker);
    }
    if leaf.seat >= epoch.maker_count || leaf.weight == 0 {
        return Err(FrozenClaimError::InvalidSeat);
    }
    if view
        .is_claimed(context.epoch_data, leaf.seat)
        .map_err(|_| FrozenClaimError::InvalidSeat)?
    {
        return Err(FrozenClaimError::AlreadyClaimed);
    }

    // A member cannot claim more than the entire epoch's asset entitlement.
    // Global allocation correctness still depends on a trusted epoch sealer.
    if leaf
        .amounts
        .iter()
        .zip(epoch.totals)
        .any(|(claim, total)| *claim > total)
    {
        return Err(FrozenClaimError::InvalidAmount);
    }
    let base = if epoch.side == 0 {
        leaf.amounts[0]
    } else {
        leaf.amounts[1]
    };
    if base == 0
        || (epoch.side == 0
            && (leaf.amounts[1] != 0 || leaf.amounts[2] != 0 || leaf.amounts[4] != 0))
        || (epoch.side == 1
            && (leaf.amounts[0] != 0 || leaf.amounts[3] != 0 || leaf.amounts[5] != 0))
    {
        return Err(FrozenClaimError::InvalidAmount);
    }

    let mut current = pubkey_claim_leaf_hash(&epoch.commitment, leaf)?;
    let mut count = epoch.maker_count;
    let mut index = leaf.seat;
    let mut depth = 0usize;
    while count > 1 {
        let sibling = siblings.get(depth).ok_or(FrozenClaimError::InvalidProof)?;
        current = if index.is_multiple_of(2) {
            if index + 1 == count && *sibling != current {
                return Err(FrozenClaimError::InvalidProof);
            }
            pubkey_claim_node_hash(&current, sibling)
        } else {
            pubkey_claim_node_hash(sibling, &current)
        };
        index /= 2;
        count = count.div_ceil(2);
        depth += 1;
    }
    if depth != siblings.len() || current != epoch.claim_root {
        return Err(FrozenClaimError::InvalidProof);
    }
    Ok(leaf.seat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hybrid_state::frozen_epoch::{FrozenEpochHeader, FROZEN_EPOCH_BYTES};

    fn fixture(root: [u8; 32], count: u32) -> ([u8; FROZEN_EPOCH_BYTES], MakerBalance) {
        let mut data = [0; FROZEN_EPOCH_BYTES];
        FrozenEpochHeader {
            bump: 1,
            side: 0,
            market: [1; 32],
            authority: [9; 32],
            program: [2; 32],
            genesis: [3; 32],
            epoch_id: 5,
            tick_sqrt_price_x64: 1u128 << 64,
            commitment: [4; 32],
            claim_root: root,
            maker_count: count,
            totals: [10, 0, 0, 30, 0, 3],
            base_vault: [7; 32],
            quote_vault: [8; 32],
        }
        .initialize(&mut data)
        .unwrap();
        (data, MakerBalance::new(1, [1; 32], [5; 32]))
    }

    #[test]
    fn single_seat_checks_signer_epoch_and_replay() {
        let leaf = PubkeyClaimLeaf {
            seat: 0,
            maker: [5; 32],
            weight: 10,
            amounts: [4, 0, 0, 12, 0, 1],
        };
        let root = pubkey_claim_leaf_hash(&[4; 32], &leaf).unwrap();
        let hex = |value: [u8; 32]| -> String {
            value.iter().map(|byte| format!("{byte:02x}")).collect()
        };
        assert_eq!(
            hex(root),
            "dd947a836deb66eec48699d9852f13979a08092a3831b707d87bb23defde0a84"
        );
        assert_eq!(
            hex(pubkey_claim_node_hash(&root, &root)),
            "58727584703157c37930af764088f159261d7dbfa9ee1f94a0d7dc840dc490c2"
        );

        let (mut data, balance) = fixture(root, 1);
        let context = ClaimContext {
            program: [2; 32],
            market: [1; 32],
            genesis: [3; 32],
            signer: [5; 32],
            is_signer: true,
            maker_balance: &balance,
            epoch_data: &data,
        };
        assert_eq!(verify_frozen_claim(&context, &leaf, &[]), Ok(0));
        assert_eq!(
            verify_frozen_claim(
                &ClaimContext {
                    is_signer: false,
                    ..context
                },
                &leaf,
                &[]
            ),
            Err(FrozenClaimError::UnauthorizedMaker)
        );
        assert_eq!(
            verify_frozen_claim(
                &ClaimContext {
                    program: [9; 32],
                    ..context
                },
                &leaf,
                &[]
            ),
            Err(FrozenClaimError::WrongProgram)
        );
        assert_eq!(
            verify_frozen_claim(
                &ClaimContext {
                    genesis: [9; 32],
                    ..context
                },
                &leaf,
                &[]
            ),
            Err(FrozenClaimError::WrongCluster)
        );
        assert_eq!(
            verify_frozen_claim(
                &ClaimContext {
                    market: [9; 32],
                    ..context
                },
                &leaf,
                &[]
            ),
            Err(FrozenClaimError::WrongMarket)
        );
        assert_eq!(
            verify_frozen_claim(
                &context,
                &PubkeyClaimLeaf {
                    amounts: [11, 0, 0, 12, 0, 1],
                    ..leaf
                },
                &[]
            ),
            Err(FrozenClaimError::InvalidAmount)
        );
        assert_eq!(
            verify_frozen_claim(
                &context,
                &PubkeyClaimLeaf {
                    maker: [6; 32],
                    ..leaf
                },
                &[]
            ),
            Err(FrozenClaimError::UnauthorizedMaker)
        );
        assert_eq!(
            verify_frozen_claim(
                &context,
                &PubkeyClaimLeaf {
                    amounts: [0, 0, 0, 0, 0, 0],
                    ..leaf
                },
                &[]
            ),
            Err(FrozenClaimError::InvalidAmount)
        );
        let view = FrozenEpochView::decode(&data).unwrap();
        view.mark_claimed(&mut data, 0).unwrap();
        let claimed = ClaimContext {
            program: [2; 32],
            market: [1; 32],
            genesis: [3; 32],
            signer: [5; 32],
            is_signer: true,
            maker_balance: &balance,
            epoch_data: &data,
        };
        assert_eq!(
            verify_frozen_claim(&claimed, &leaf, &[]),
            Err(FrozenClaimError::AlreadyClaimed)
        );
    }

    #[test]
    fn three_seat_proof_cannot_forge_odd_padding_or_amounts() {
        let leaves = [
            PubkeyClaimLeaf {
                seat: 0,
                maker: [5; 32],
                weight: 3,
                amounts: [2, 0, 0, 6, 0, 1],
            },
            PubkeyClaimLeaf {
                seat: 1,
                maker: [6; 32],
                weight: 3,
                amounts: [3, 0, 0, 9, 0, 1],
            },
            PubkeyClaimLeaf {
                seat: 2,
                maker: [7; 32],
                weight: 4,
                amounts: [5, 0, 0, 15, 0, 1],
            },
        ];
        let hashes = leaves.map(|l| pubkey_claim_leaf_hash(&[4; 32], &l).unwrap());
        let left = pubkey_claim_node_hash(&hashes[0], &hashes[1]);
        let right = pubkey_claim_node_hash(&hashes[2], &hashes[2]);
        let root = pubkey_claim_node_hash(&left, &right);
        let (data, _) = fixture(root, 3);
        let balance = MakerBalance::new(1, [1; 32], [7; 32]);
        let context = ClaimContext {
            program: [2; 32],
            market: [1; 32],
            genesis: [3; 32],
            signer: [7; 32],
            is_signer: true,
            maker_balance: &balance,
            epoch_data: &data,
        };
        let siblings = [hashes[2], left];
        assert_eq!(verify_frozen_claim(&context, &leaves[2], &siblings), Ok(2));
        let forged = [[0xaa; 32], left];
        assert_eq!(
            verify_frozen_claim(&context, &leaves[2], &forged),
            Err(FrozenClaimError::InvalidProof)
        );
        let changed = PubkeyClaimLeaf {
            amounts: [5, 0, 0, 15, 0, 2],
            ..leaves[2]
        };
        assert_eq!(
            verify_frozen_claim(&context, &changed, &siblings),
            Err(FrozenClaimError::InvalidProof)
        );
        assert_eq!(
            verify_frozen_claim(&context, &leaves[2], &siblings[..1]),
            Err(FrozenClaimError::InvalidProof)
        );
    }
}
