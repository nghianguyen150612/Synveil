//! Test-only filesystem cleanup helpers.
//!
//! Windows keeps SQLite WAL/SHM descriptors and watcher handles open until
//! the last handle is dropped. A plain `fs::remove_dir_all` immediately after
//! `drop` can therefore hit ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION
//! (33). These helpers make teardown deterministic without touching production
//! locking or durability.

use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

const MAX_CLEANUP_ATTEMPTS: u32 = 40;
const CLEANUP_RETRY_DELAY_MS: u64 = 25;

fn is_windows_lock_violation(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))
}

/// Bounded Windows-safe directory removal. On sharing/lock violation, retries
/// with a small fixed backoff; any other IO error fails immediately. On
/// non-Windows targets this is a plain remove.
#[allow(dead_code)]
pub(crate) fn remove_dir_all_bounded(path: &Path) -> io::Result<()> {
    if !cfg!(windows) {
        return std::fs::remove_dir_all(path);
    }
    let deadline = Instant::now()
        + Duration::from_millis(CLEANUP_RETRY_DELAY_MS * u64::from(MAX_CLEANUP_ATTEMPTS));
    loop {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error) if is_windows_lock_violation(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(CLEANUP_RETRY_DELAY_MS));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Bounded Windows-safe file read. SQLite keeps WAL/SHM descriptors open while
/// the pool is alive, so verifying "no plaintext secret on disk" may
/// transiently hit a sharing/lock violation on Windows. Retries only those
/// specific errors; every other IO error fails immediately.
#[allow(dead_code)]
pub(crate) fn read_file_bounded(path: &Path) -> io::Result<Vec<u8>> {
    if !cfg!(windows) {
        return std::fs::read(path);
    }
    let deadline = Instant::now()
        + Duration::from_millis(CLEANUP_RETRY_DELAY_MS * u64::from(MAX_CLEANUP_ATTEMPTS));
    loop {
        match std::fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if is_windows_lock_violation(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(CLEANUP_RETRY_DELAY_MS));
            }
            Err(error) => return Err(error),
        }
    }
}
