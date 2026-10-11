//! Filesystem persistence for the host-only maker submission journal.
//! Operators must serialize access to this file across processes. A missing
//! record is not proof that a previously broadcast transaction never landed.
use crate::client_journal::{decode_journal, encode_journal, JournalError, JOURNAL_BYTES};
use crate::client_submission::SubmissionTracker;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Corrupt(JournalError),
    Missing,
}
impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Load a journal exactly; never default an unreadable or absent journal to
/// an idle tracker. Operators must explicitly initialize a fresh market.
pub fn load(path: &Path) -> Result<SubmissionTracker, StoreError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(StoreError::Missing);
        }
        Err(error) => return Err(StoreError::Io(error)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    decode_journal(&bytes).map_err(StoreError::Corrupt)
}

/// Atomically replace a journal via a same-directory temporary file.
/// A leftover temporary file is intentionally an error until an operator
/// reconciles it; silently discarding it could permit duplicate submission.
pub fn persist(path: &Path, tracker: &SubmissionTracker) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::Io(io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))
    })?;
    let name = path.file_name().ok_or_else(|| {
        StoreError::Io(io::Error::new(io::ErrorKind::InvalidInput, "missing filename"))
    })?;
    let mut temporary_name = name.to_os_string();
    temporary_name.push(".pending");
    let temporary = parent.join(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let bytes = encode_journal(tracker);
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Create an idle journal only when the caller has independently established
/// there is no pending transaction. This intentionally cannot overwrite a
/// pre-existing journal or bypass a leftover temporary file.
pub fn initialize(path: &Path) -> Result<(), StoreError> {
    if path.exists() {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "journal already exists",
        )));
    }
    persist(path, &SubmissionTracker::default())
}

pub const JOURNAL_RECORD_BYTES: usize = JOURNAL_BYTES;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_submission::SubmissionPhase;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(1);

    #[test]
    fn durable_journal_roundtrips_and_fails_closed_on_corruption() {
        let directory = std::env::temp_dir().join(format!(
            "hybrid-mm-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("maker.journal");
        assert!(matches!(load(&path), Err(StoreError::Missing)));
        initialize(&path).unwrap();
        assert!(matches!(
            initialize(&path),
            Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(load(&path).unwrap(), SubmissionTracker::default());
        let state = SubmissionTracker {
            phase: SubmissionPhase::Submitted {
                signature: [8; 64],
                last_valid_block_height: 100,
            },
            last_confirmed_slot: 20,
        };
        persist(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), state);
        fs::write(&path, b"corrupt").unwrap();
        assert!(matches!(load(&path), Err(StoreError::Corrupt(_))));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn unresolved_temporary_record_blocks_replace() {
        let directory = std::env::temp_dir().join(format!(
            "hybrid-mm-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("maker.journal");
        fs::write(directory.join("maker.journal.pending"), b"unresolved").unwrap();
        assert!(matches!(
            persist(&path, &SubmissionTracker::default()),
            Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        fs::remove_dir_all(&directory).unwrap();
    }
}
