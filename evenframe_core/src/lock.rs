//! Cross-process serialization of evenframe runs.

use crate::error::{EvenframeError, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions, TryLockError};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// A per-project lock held for the lifetime of an evenframe run.
///
/// Concurrent runs against the same project mutate shared state — the
/// database schema, generated type files, and the `.evenframe` caches — so a
/// second process blocks until the first finishes instead of interleaving
/// with it. The lock is an OS advisory file lock: the kernel releases it when
/// the holder exits (cleanly or not), so a crashed run cannot leave a stale
/// lock behind.
///
/// The lock file lives in the system temp directory, keyed by the
/// canonicalized project root, so committed `.evenframe/` directories stay
/// free of runtime artifacts.
pub struct ProcessLock {
    /// Holding the handle holds the kernel lock; releasing happens on drop
    /// (or process death) when the descriptor closes.
    _file: File,
}

impl ProcessLock {
    /// Acquires the lock for `project_root`, blocking until any other
    /// evenframe process releases it. Logs when it has to wait.
    pub fn acquire(project_root: &Path) -> Result<Self> {
        let path = Self::lock_path(project_root);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| {
                EvenframeError::config(format!(
                    "Failed to open process lock {}: {e}",
                    path.display()
                ))
            })?;

        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                // Straight to stderr, not just tracing: at the CLI's default
                // filter level an info! line is invisible, and a silent hang
                // while waiting would look like a freeze.
                eprintln!(
                    "Another evenframe process is running against this project; waiting for it to finish (lock: {})",
                    path.display()
                );
                info!(
                    lock = %path.display(),
                    "Another evenframe process is running against this project; waiting for it to finish"
                );
                file.lock().map_err(|e| {
                    EvenframeError::config(format!(
                        "Failed waiting for process lock {}: {e}",
                        path.display()
                    ))
                })?;
            }
            Err(TryLockError::Error(e)) => {
                return Err(EvenframeError::config(format!(
                    "Failed to acquire process lock {}: {e}",
                    path.display()
                )));
            }
        }

        // Best-effort diagnostics: record the holder's pid in the lock file.
        let _ = file.set_len(0);
        let _ = writeln!(&file, "{}", std::process::id());

        debug!(lock = %path.display(), "Acquired evenframe process lock");
        Ok(Self { _file: file })
    }

    /// The lock-file path for a project root: stable per canonical root,
    /// outside the repository.
    fn lock_path(project_root: &Path) -> PathBuf {
        let canonical = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        std::env::temp_dir().join(format!("evenframe-{:016x}.lock", hasher.finish()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn lock_path_is_stable_for_a_root() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            ProcessLock::lock_path(dir.path()),
            ProcessLock::lock_path(dir.path())
        );
    }

    #[test]
    fn lock_paths_differ_between_roots() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        assert_ne!(
            ProcessLock::lock_path(a.path()),
            ProcessLock::lock_path(b.path())
        );
    }

    #[test]
    fn held_lock_blocks_other_handles_until_released() {
        let dir = TempDir::new().unwrap();
        let lock = ProcessLock::acquire(dir.path()).unwrap();

        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(ProcessLock::lock_path(dir.path()))
            .unwrap();
        assert!(
            matches!(probe.try_lock(), Err(TryLockError::WouldBlock)),
            "a second handle must not acquire the lock while it is held"
        );

        drop(lock);
        assert!(
            probe.try_lock().is_ok(),
            "the lock must be free once the holder is dropped"
        );
    }
}
