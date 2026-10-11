//! Host-only submission state machine for restart-safe market-maker operations.
//! A signature is not a confirmation. Never resubmit a non-idempotent maker
//! mutation until the previous signature is conclusively expired or finalized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionPhase {
    Idle,
    Prepared {
        snapshot_slot: u64,
    },
    Submitted {
        signature: [u8; 64],
        last_valid_block_height: u64,
    },
    Confirmed {
        signature: [u8; 64],
        confirmed_slot: u64,
    },
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionError {
    Pending,
    StaleSnapshot,
    NotPrepared,
    WrongSignature,
    NotExpired,
    InvalidConfirmation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubmissionTracker {
    pub phase: SubmissionPhase,
    pub last_confirmed_slot: u64,
}

impl Default for SubmissionTracker {
    fn default() -> Self {
        Self {
            phase: SubmissionPhase::Idle,
            last_confirmed_slot: 0,
        }
    }
}

impl SubmissionTracker {
    /// Caller must persist this state before broadcasting a signature.
    pub fn prepare(&mut self, snapshot_slot: u64) -> Result<(), SubmissionError> {
        if matches!(
            self.phase,
            SubmissionPhase::Prepared { .. } | SubmissionPhase::Submitted { .. }
        ) {
            return Err(SubmissionError::Pending);
        }
        if snapshot_slot < self.last_confirmed_slot {
            return Err(SubmissionError::StaleSnapshot);
        }
        self.phase = SubmissionPhase::Prepared { snapshot_slot };
        Ok(())
    }

    pub fn submitted(
        &mut self,
        signature: [u8; 64],
        last_valid_block_height: u64,
    ) -> Result<(), SubmissionError> {
        if !matches!(self.phase, SubmissionPhase::Prepared { .. }) {
            return Err(SubmissionError::NotPrepared);
        }
        self.phase = SubmissionPhase::Submitted {
            signature,
            last_valid_block_height,
        };
        Ok(())
    }

    pub fn confirm(
        &mut self,
        signature: [u8; 64],
        finalized_slot: u64,
    ) -> Result<(), SubmissionError> {
        let SubmissionPhase::Submitted {
            signature: submitted,
            ..
        } = self.phase
        else {
            return Err(SubmissionError::NotPrepared);
        };
        if signature != submitted {
            return Err(SubmissionError::WrongSignature);
        }
        if finalized_slot < self.last_confirmed_slot {
            return Err(SubmissionError::InvalidConfirmation);
        }
        self.last_confirmed_slot = finalized_slot;
        self.phase = SubmissionPhase::Confirmed {
            signature,
            confirmed_slot: finalized_slot,
        };
        Ok(())
    }

    /// Only unlock after blockhash expiration AND an authoritative signature
    /// status check confirms that no successful transaction landed.
    pub fn expire(
        &mut self,
        current_block_height: u64,
        signature_absent: bool,
    ) -> Result<(), SubmissionError> {
        let SubmissionPhase::Submitted {
            last_valid_block_height,
            ..
        } = self.phase
        else {
            return Err(SubmissionError::NotPrepared);
        };
        if current_block_height <= last_valid_block_height || !signature_absent {
            return Err(SubmissionError::NotExpired);
        }
        self.phase = SubmissionPhase::Expired;
        Ok(())
    }

    pub fn has_pending_mutation(&self) -> bool {
        matches!(
            self.phase,
            SubmissionPhase::Prepared { .. } | SubmissionPhase::Submitted { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfirmed_submissions_block_requote_and_retry() {
        let mut tracker = SubmissionTracker::default();
        tracker.prepare(20).unwrap();
        assert_eq!(tracker.prepare(21), Err(SubmissionError::Pending));
        tracker.submitted([7; 64], 90).unwrap();
        assert!(tracker.has_pending_mutation());
        assert_eq!(tracker.prepare(22), Err(SubmissionError::Pending));
        assert_eq!(tracker.expire(91, false), Err(SubmissionError::NotExpired));
        assert_eq!(tracker.expire(90, true), Err(SubmissionError::NotExpired));
        assert_eq!(
            tracker.confirm([8; 64], 22),
            Err(SubmissionError::WrongSignature)
        );
        tracker.confirm([7; 64], 22).unwrap();
        assert!(!tracker.has_pending_mutation());
        assert_eq!(tracker.prepare(21), Err(SubmissionError::StaleSnapshot));
        tracker.prepare(22).unwrap();
    }

    #[test]
    fn expiry_needs_both_height_and_absent_signature() {
        let mut tracker = SubmissionTracker::default();
        tracker.prepare(1).unwrap();
        tracker.submitted([9; 64], 15).unwrap();
        assert_eq!(tracker.expire(16, false), Err(SubmissionError::NotExpired));
        tracker.expire(16, true).unwrap();
        assert!(!tracker.has_pending_mutation());
        tracker.prepare(2).unwrap();
    }
}
