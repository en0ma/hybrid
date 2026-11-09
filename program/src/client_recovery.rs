//! Host-only recovery decision for the durable maker submission journal.
//! Never infer finality from a sent signature, a mere confirmed commitment,
//! timeout, or an RPC "not found" response without authoritative history.
//! Does not build, sign, send, or simulate transactions.
use crate::client_session::{MakerSession, SessionError};
use crate::client_submission::SubmissionPhase;

/// RPC adapters must authenticate results for the exact cluster and signature,
/// query finalized commitment, and use transaction history when proving absence.
/// "NotFound" is not by itself evidence of expiry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureObservation {
    Pending,
    FinalizedSuccess { signature: [u8; 64], slot: u64 },
    FinalizedFailure { signature: [u8; 64] },
    NotFound { history_complete: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    NothingPending,
    PreparedNeedsIntervention,
    StillPending,
    Finalized { slot: u64 },
    Expired,
}

#[derive(Debug)]
pub enum RecoveryError {
    Session(SessionError),
    WrongSignature,
    FinalizedFailure,
    UnknownHistory,
    NotExpired,
}

impl From<SessionError> for RecoveryError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

/// Applies a trusted observation of the *recorded* signature only.
/// A Submitted transaction may be unlocked in exactly two ways:
/// finalized successful execution, or blockhash expiry plus independently
/// verified complete absence from transaction history. A transaction that
/// finalized with an error is held for operator reconciliation instead.
/// The caller MUST refresh maker accounts before preparing another mutation.
pub fn reconcile_submission(
    session: &mut MakerSession,
    observation: SignatureObservation,
    current_block_height: u64,
) -> Result<RecoveryOutcome, RecoveryError> {
    let SubmissionPhase::Submitted {
        signature,
        last_valid_block_height,
    } = session.tracker().phase
    else {
        return match session.tracker().phase {
            SubmissionPhase::Prepared { .. } => Ok(RecoveryOutcome::PreparedNeedsIntervention),
            _ => Ok(RecoveryOutcome::NothingPending),
        };
    };
    match observation {
        SignatureObservation::Pending => Ok(RecoveryOutcome::StillPending),
        SignatureObservation::FinalizedSuccess {
            signature: observed,
            slot,
        } => {
            if observed != signature {
                return Err(RecoveryError::WrongSignature);
            }
            session.confirm(signature, slot)?;
            Ok(RecoveryOutcome::Finalized { slot })
        }
        SignatureObservation::FinalizedFailure {
            signature: observed,
        } => {
            if observed != signature {
                return Err(RecoveryError::WrongSignature);
            }
            Err(RecoveryError::FinalizedFailure)
        }
        SignatureObservation::NotFound {
            history_complete: false,
        } => Err(RecoveryError::UnknownHistory),
        SignatureObservation::NotFound {
            history_complete: true,
        } => {
            if current_block_height <= last_valid_block_height {
                return Err(RecoveryError::NotExpired);
            }
            session.expire(current_block_height, true)?;
            Ok(RecoveryOutcome::Expired)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_store::initialize;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(1);

    fn with_session(test: impl FnOnce(&mut MakerSession, &std::path::Path)) {
        let folder = std::env::temp_dir().join(format!(
            "hybrid-maker-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("journal");
        initialize(&path).unwrap();
        let mut session = MakerSession::open(&path).unwrap();
        session.prepare(100).unwrap();
        session.mark_submitted([3; 64], 150).unwrap();
        test(&mut session, &path);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn unknown_status_or_incomplete_history_cannot_unlock_pending_mutation() {
        with_session(|session, path| {
            assert_eq!(
                reconcile_submission(session, SignatureObservation::Pending, 151).unwrap(),
                RecoveryOutcome::StillPending
            );
            assert!(matches!(
                reconcile_submission(
                    session,
                    SignatureObservation::NotFound {
                        history_complete: false,
                    },
                    999,
                ),
                Err(RecoveryError::UnknownHistory)
            ));
            assert!(matches!(
                reconcile_submission(
                    session,
                    SignatureObservation::NotFound {
                        history_complete: true,
                    },
                    150,
                ),
                Err(RecoveryError::NotExpired)
            ));
            assert!(session.can_requote().is_err());
            assert!(MakerSession::open(path).unwrap().can_requote().is_err());
        });
    }

    #[test]
    fn finalized_success_is_persisted_and_restart_allows_new_snapshot() {
        with_session(|session, path| {
            assert!(matches!(
                reconcile_submission(
                    session,
                    SignatureObservation::FinalizedSuccess {
                        signature: [4; 64],
                        slot: 101,
                    },
                    200,
                ),
                Err(RecoveryError::WrongSignature)
            ));
            assert_eq!(
                reconcile_submission(
                    session,
                    SignatureObservation::FinalizedSuccess {
                        signature: [3; 64],
                        slot: 102,
                    },
                    120,
                )
                .unwrap(),
                RecoveryOutcome::Finalized { slot: 102 }
            );
            let mut resumed = MakerSession::open(path).unwrap();
            assert!(resumed.can_requote().is_ok());
            assert!(resumed.prepare(101).is_err());
            resumed.prepare(102).unwrap();
        });
    }

    #[test]
    fn only_complete_history_and_expired_height_can_release_a_signature() {
        with_session(|session, path| {
            assert!(matches!(
                reconcile_submission(
                    session,
                    SignatureObservation::FinalizedFailure { signature: [3; 64] },
                    999,
                ),
                Err(RecoveryError::FinalizedFailure)
            ));
            assert!(session.can_requote().is_err());
            assert_eq!(
                reconcile_submission(
                    session,
                    SignatureObservation::NotFound {
                        history_complete: true,
                    },
                    151,
                )
                .unwrap(),
                RecoveryOutcome::Expired
            );
            assert!(MakerSession::open(path).unwrap().can_requote().is_ok());
        });
    }

    #[test]
    fn prepared_without_signature_requires_explicit_intervention() {
        let folder = std::env::temp_dir().join(format!(
            "hybrid-maker-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("journal");
        initialize(&path).unwrap();
        let mut session = MakerSession::open(&path).unwrap();
        session.prepare(100).unwrap();
        assert_eq!(
            reconcile_submission(&mut session, SignatureObservation::Pending, 100).unwrap(),
            RecoveryOutcome::PreparedNeedsIntervention
        );
        assert!(MakerSession::open(&path).unwrap().can_requote().is_err());
        fs::remove_dir_all(folder).unwrap();
    }
}
