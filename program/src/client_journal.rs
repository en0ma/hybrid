//! Durable, versioned encoding for a maker's in-flight transaction state.
//! Persist atomically (write, fsync, rename) before network broadcast.
//! Unknown or corrupt records fail closed and must never be treated as idle.
use crate::client_submission::{SubmissionPhase, SubmissionTracker};

const MAGIC: &[u8; 8] = b"HYBMMJ01";
pub const JOURNAL_BYTES: usize = 104;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalError {
    InvalidLength,
    InvalidMagic,
    InvalidPhase,
    InvalidChecksum,
    InvalidFields,
}

fn checksum(bytes: &[u8]) -> u64 {
    // Integrity detector, not an authentication MAC. The journal file must
    // also be protected by OS filesystem permissions.
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

pub fn encode_journal(tracker: &SubmissionTracker) -> [u8; JOURNAL_BYTES] {
    let mut out = [0u8; JOURNAL_BYTES];
    out[..8].copy_from_slice(MAGIC);
    out[8..16].copy_from_slice(&tracker.last_confirmed_slot.to_le_bytes());
    let (tag, slot_or_height, signature) = match tracker.phase {
        SubmissionPhase::Idle => (0, 0, [0; 64]),
        SubmissionPhase::Prepared { snapshot_slot } => (1, snapshot_slot, [0; 64]),
        SubmissionPhase::Submitted {
            signature,
            last_valid_block_height,
        } => (2, last_valid_block_height, signature),
        SubmissionPhase::Confirmed {
            signature,
            confirmed_slot,
        } => (3, confirmed_slot, signature),
        SubmissionPhase::Expired => (4, 0, [0; 64]),
    };
    out[16] = tag;
    out[17..25].copy_from_slice(&slot_or_height.to_le_bytes());
    out[25..89].copy_from_slice(&signature);
    let digest = checksum(&out[..96]);
    out[96..104].copy_from_slice(&digest.to_le_bytes());
    out
}

pub fn decode_journal(bytes: &[u8]) -> Result<SubmissionTracker, JournalError> {
    if bytes.len() != JOURNAL_BYTES {
        return Err(JournalError::InvalidLength);
    }
    if &bytes[..8] != MAGIC {
        return Err(JournalError::InvalidMagic);
    }
    let actual = u64::from_le_bytes(bytes[96..104].try_into().unwrap());
    if actual != checksum(&bytes[..96]) {
        return Err(JournalError::InvalidChecksum);
    }
    if bytes[89..96].iter().any(|byte| *byte != 0) {
        return Err(JournalError::InvalidFields);
    }
    let last_confirmed_slot = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let value = u64::from_le_bytes(bytes[17..25].try_into().unwrap());
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&bytes[25..89]);
    let phase = match bytes[16] {
        0 if value == 0 && signature == [0; 64] => SubmissionPhase::Idle,
        1 if signature == [0; 64] => SubmissionPhase::Prepared {
            snapshot_slot: value,
        },
        2 => SubmissionPhase::Submitted {
            signature,
            last_valid_block_height: value,
        },
        3 if value >= last_confirmed_slot => SubmissionPhase::Confirmed {
            signature,
            confirmed_slot: value,
        },
        4 if value == 0 && signature == [0; 64] => SubmissionPhase::Expired,
        0..=4 => return Err(JournalError::InvalidFields),
        _ => return Err(JournalError::InvalidPhase),
    };
    Ok(SubmissionTracker {
        phase,
        last_confirmed_slot,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_roundtrips_all_phases_and_rejects_corruption() {
        let phases = [
            SubmissionPhase::Idle,
            SubmissionPhase::Prepared { snapshot_slot: 42 },
            SubmissionPhase::Submitted {
                signature: [9; 64],
                last_valid_block_height: 90,
            },
            SubmissionPhase::Confirmed {
                signature: [7; 64],
                confirmed_slot: 51,
            },
            SubmissionPhase::Expired,
        ];
        for phase in phases {
            let tracker = SubmissionTracker {
                phase,
                last_confirmed_slot: 20,
            };
            let bytes = encode_journal(&tracker);
            assert_eq!(decode_journal(&bytes), Ok(tracker));
            let mut damaged = bytes;
            damaged[25] ^= 1;
            assert_eq!(
                decode_journal(&damaged),
                Err(JournalError::InvalidChecksum)
            );
            assert_eq!(
                decode_journal(&bytes[..103]),
                Err(JournalError::InvalidLength)
            );
        }
    }

    #[test]
    fn unknown_phase_fails_closed_even_with_matching_checksum() {
        let mut bytes = encode_journal(&SubmissionTracker::default());
        bytes[16] = 255;
        let digest = checksum(&bytes[..96]);
        bytes[96..104].copy_from_slice(&digest.to_le_bytes());
        assert_eq!(decode_journal(&bytes), Err(JournalError::InvalidPhase));
    }
}
