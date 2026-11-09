//! Versioned, read-only frozen maker claim Merkle proof verification.
//! This library is NOT wired to Hybrid's deployed settlement processor.
//! Inclusion proves only membership under a caller-authenticated root;
//! it does not prove fill accounting, PDA ownership, solvency or entitlement.
//! V1 binary bytes exactly match sdk/src/pro-rata-merkle.mjs.
use sha2::{Digest, Sha256};

pub const MAX_EPOCH_MAKERS: u32 = 4096;
const LEAF_DOMAIN: &[u8] = b"HYBRID_LAZY_MAKER_LEAF_V1\0";
const NODE_DOMAIN: &[u8] = b"HYBRID_LAZY_MAKER_NODE_V1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimProofError {
    InvalidIndex,
    InvalidCount,
    InvalidMaker,
    ZeroWeight,
    InvalidPath,
    RootMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MakerClaimLeaf<'a> {
    pub index: u32,
    /// Temporary research ABI: restricted ASCII ID, not Solana signer/PDA.
    pub maker_id: &'a str,
    pub weight: u64,
    /// baseDebit, baseCredit, quoteDebit, quoteCredit, baseFee, quoteFee.
    pub amounts: [u64; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MakerClaimProof<'a> {
    pub leaf_count: u32,
    pub leaf: MakerClaimLeaf<'a>,
    /// Siblings from the leaf towards the root; duplicate final nodes when
    /// odd, matching the JS proof's canonical padding rule.
    pub siblings: &'a [[u8; 32]],
}

pub fn claim_leaf_hash(
    epoch_commitment: &[u8; 32],
    leaf: &MakerClaimLeaf<'_>,
) -> Result<[u8; 32], ClaimProofError> {
    if leaf.index >= MAX_EPOCH_MAKERS {
        return Err(ClaimProofError::InvalidIndex);
    }
    let name = leaf.maker_id.as_bytes();
    if name.is_empty()
        || name.len() > 128
        || !name
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
    {
        return Err(ClaimProofError::InvalidMaker);
    }
    if leaf.weight == 0 {
        return Err(ClaimProofError::ZeroWeight);
    }
    let mut hasher = Sha256::new();
    hasher.update(LEAF_DOMAIN);
    hasher.update(epoch_commitment);
    hasher.update(leaf.index.to_le_bytes());
    hasher.update([name.len() as u8]);
    hasher.update(name);
    hasher.update(leaf.weight.to_le_bytes());
    for amount in leaf.amounts {
        hasher.update(amount.to_le_bytes());
    }
    Ok(hasher.finalize().into())
}

pub fn claim_node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(NODE_DOMAIN);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Verify bounded O(log N) inclusion against an independently authenticated
/// epoch root, immutable epoch digest and exact expected maker ID.
/// No SPL transfer or claim-state mutation is performed.
pub fn verify_maker_claim(
    expected_root: &[u8; 32],
    epoch_commitment: &[u8; 32],
    expected_maker_id: &str,
    proof: &MakerClaimProof<'_>,
) -> Result<(), ClaimProofError> {
    let mut count = proof.leaf_count;
    let mut index = proof.leaf.index;
    if count == 0 || count > MAX_EPOCH_MAKERS {
        return Err(ClaimProofError::InvalidCount);
    }
    if index >= count {
        return Err(ClaimProofError::InvalidIndex);
    }
    if proof.leaf.maker_id != expected_maker_id {
        return Err(ClaimProofError::InvalidMaker);
    }
    let mut hash = claim_leaf_hash(epoch_commitment, &proof.leaf)?;
    let mut depth = 0usize;
    while count > 1 {
        let sibling = proof
            .siblings
            .get(depth)
            .ok_or(ClaimProofError::InvalidPath)?;
        hash = if index % 2 == 0 {
            // Terminal odd leaf must be paired with itself, not arbitrary
            // outsider data representing a phantom roster position.
            if index + 1 >= count && *sibling != hash {
                return Err(ClaimProofError::InvalidPath);
            }
            claim_node_hash(&hash, sibling)
        } else {
            claim_node_hash(sibling, &hash)
        };
        index /= 2;
        count = count.div_ceil(2);
        depth += 1;
    }
    if depth != proof.siblings.len() {
        return Err(ClaimProofError::InvalidPath);
    }
    if hash != *expected_root {
        return Err(ClaimProofError::RootMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Calculated independently in Node.js from the frozen v1 binary layout.
    const SAMPLE_LEAF_HASH: &str =
        "ae73290b47ed38b51dbc93ef0a058240e82a8d3eacadb43bf2614a787464201a";
    const SAMPLE_NODE_HASH: &str =
        "d68609120b2b28233b897f0829e4dbd01f9566a25c0b7a1d87fb07af904271f8";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    fn sample() -> MakerClaimLeaf<'static> {
        MakerClaimLeaf {
            index: 2,
            maker_id: "maker_A",
            weight: 3,
            amounts: [2, 0, 0, 7, 0, 1],
        }
    }
    #[test]
    fn exact_js_binary_leaf_and_node_vectors() {
        assert_eq!(
            hex(&claim_leaf_hash(&[0x11; 32], &sample()).unwrap()),
            SAMPLE_LEAF_HASH
        );
        assert_eq!(
            hex(&claim_node_hash(&[0xaa; 32], &[0xbb; 32])),
            SAMPLE_NODE_HASH
        );
    }
    #[test]
    fn singleton_proof_and_failed_replays() {
        let leaf = MakerClaimLeaf {
            index: 0,
            ..sample()
        };
        let digest = [0x11; 32];
        let root = claim_leaf_hash(&digest, &leaf).unwrap();
        let proof = MakerClaimProof {
            leaf_count: 1,
            leaf,
            siblings: &[],
        };
        assert_eq!(
            verify_maker_claim(&root, &digest, "maker_A", &proof),
            Ok(())
        );
        assert_eq!(
            verify_maker_claim(&root, &digest, "attacker", &proof),
            Err(ClaimProofError::InvalidMaker)
        );
        assert_eq!(
            verify_maker_claim(&root, &[0x22; 32], "maker_A", &proof),
            Err(ClaimProofError::RootMismatch)
        );
        let extra = [[0u8; 32]];
        let wrong = MakerClaimProof {
            siblings: &extra,
            ..proof
        };
        assert_eq!(
            verify_maker_claim(&root, &digest, "maker_A", &wrong),
            Err(ClaimProofError::InvalidPath)
        );
    }
    #[test]
    fn odd_leaf_proof_rejects_fake_padding_and_wrong_root() {
        let digest = [0x11; 32];
        let a = MakerClaimLeaf {
            index: 0,
            maker_id: "A",
            weight: 1,
            amounts: [1, 0, 0, 2, 0, 0],
        };
        let b = MakerClaimLeaf {
            index: 1,
            maker_id: "B",
            weight: 1,
            amounts: [0, 0, 0, 0, 0, 0],
        };
        let c = MakerClaimLeaf {
            index: 2,
            maker_id: "C",
            weight: 1,
            amounts: [1, 0, 0, 3, 0, 1],
        };
        let ha = claim_leaf_hash(&digest, &a).unwrap();
        let hb = claim_leaf_hash(&digest, &b).unwrap();
        let hc = claim_leaf_hash(&digest, &c).unwrap();
        let left = claim_node_hash(&ha, &hb);
        let right = claim_node_hash(&hc, &hc);
        let root = claim_node_hash(&left, &right);
        assert_eq!(
            hex(&root),
            "8df5339645b4e7481bbab50a25f00805f29e28ae3002cbee1e23e1f2fafb41bc"
        );
        let siblings = [hc, left];
        let proof = MakerClaimProof {
            leaf_count: 3,
            leaf: c,
            siblings: &siblings,
        };
        assert_eq!(verify_maker_claim(&root, &digest, "C", &proof), Ok(()));
        let fake = [[0x77; 32], left];
        let corrupt = MakerClaimProof {
            siblings: &fake,
            ..proof.clone()
        };
        assert_eq!(
            verify_maker_claim(&root, &digest, "C", &corrupt),
            Err(ClaimProofError::InvalidPath)
        );
        assert_eq!(
            verify_maker_claim(&[0x44; 32], &digest, "C", &proof),
            Err(ClaimProofError::RootMismatch)
        );
    }
    #[test]
    fn deny_zero_weight_wrong_leaf_identity_and_bounds() {
        let digest = [0x11; 32];
        for leaf in [
            MakerClaimLeaf {
                weight: 0,
                ..sample()
            },
            MakerClaimLeaf {
                maker_id: "not valid",
                ..sample()
            },
            MakerClaimLeaf {
                maker_id: "",
                ..sample()
            },
        ] {
            assert!(claim_leaf_hash(&digest, &leaf).is_err());
        }
        let invalid = MakerClaimLeaf {
            index: 4096,
            ..sample()
        };
        assert_eq!(
            claim_leaf_hash(&digest, &invalid),
            Err(ClaimProofError::InvalidIndex)
        );
        let leaf = sample();
        let proof = MakerClaimProof {
            leaf_count: 2,
            leaf,
            siblings: &[],
        };
        assert_eq!(
            verify_maker_claim(&[0; 32], &digest, "maker_A", &proof),
            Err(ClaimProofError::InvalidIndex)
        );
    }
}
