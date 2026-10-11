//! Restart-aware maker session. A journaled pending signature always blocks
//! requoting until a separate finality/expiration reconciliation is completed.
use crate::client_store::{load, persist, StoreError};
use crate::client_submission::{SubmissionError, SubmissionTracker};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SessionError {
    Store(StoreError),
    Submission(SubmissionError),
    PendingRecovery,
}

pub struct MakerSession {
    path: PathBuf,
    tracker: SubmissionTracker,
}

impl MakerSession {
    /// Opening an absent or invalid journal must fail, never create Idle.
    pub fn open(path: &Path) -> Result<Self, SessionError> {
        let tracker = load(path).map_err(SessionError::Store)?;
        Ok(Self {
            path: path.to_path_buf(),
            tracker,
        })
    }

    pub fn tracker(&self) -> &SubmissionTracker {
        &self.tracker
    }

    /// Starting a new session must not silently discard Prepared/Submitted.
    pub fn can_requote(&self) -> Result<(), SessionError> {
        if self.tracker.has_pending_mutation() {
            return Err(SessionError::PendingRecovery);
        }
        Ok(())
    }

    pub fn prepare(&mut self, snapshot_slot: u64) -> Result<(), SessionError> {
        self.can_requote()?;
        let mut next = self.tracker;
        next.prepare(snapshot_slot).map_err(SessionError::Submission)?;
        persist(&self.path, &next).map_err(SessionError::Store)?;
        self.tracker = next;
        Ok(())
    }

    /// Persist the intended signature before the caller broadcasts it.
    pub fn mark_submitted(
        &mut self,
        signature: [u8; 64],
        last_valid_block_height: u64,
    ) -> Result<(), SessionError> {
        let mut next = self.tracker;
        next.submitted(signature, last_valid_block_height)
            .map_err(SessionError::Submission)?;
        persist(&self.path, &next).map_err(SessionError::Store)?;
        self.tracker = next;
        Ok(())
    }

    pub fn confirm(
        &mut self,
        signature: [u8; 64],
        finalized_slot: u64,
    ) -> Result<(), SessionError> {
        let mut next = self.tracker;
        next.confirm(signature, finalized_slot)
            .map_err(SessionError::Submission)?;
        persist(&self.path, &next).map_err(SessionError::Store)?;
        self.tracker = next;
        Ok(())
    }

    pub fn expire(
        &mut self,
        block_height: u64,
        signature_absent: bool,
    ) -> Result<(), SessionError> {
        let mut next = self.tracker;
        next.expire(block_height, signature_absent)
            .map_err(SessionError::Submission)?;
        persist(&self.path, &next).map_err(SessionError::Store)?;
        self.tracker = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_store::initialize;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(1);

    #[test]
    fn reboot_restores_unconfirmed_signature_and_blocks_quote_mutations() {
        let directory = std::env::temp_dir().join(format!(
            "hybrid-mm-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("journal");
        assert!(matches!(MakerSession::open(&path), Err(SessionError::Store(_))));
        initialize(&path).unwrap();
        let mut session = MakerSession::open(&path).unwrap();
        session.prepare(100).unwrap();
        session.mark_submitted([12; 64], 500).unwrap();
        drop(session);
        let mut resumed = MakerSession::open(&path).unwrap();
        assert!(matches!(resumed.can_requote(), Err(SessionError::PendingRecovery)));
        assert!(matches!(
            resumed.expire(501, false),
            Err(SessionError::Submission(SubmissionError::NotExpired))
        ));
        resumed.confirm([12; 64], 102).unwrap();
        assert!(resumed.can_requote().is_ok());
        resumed.prepare(102).unwrap();
        assert!(matches!(resumed.can_requote(), Err(SessionError::PendingRecovery)));
        fs::remove_dir_all(directory).unwrap();
    }
}
