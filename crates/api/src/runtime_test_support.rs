//! Shared synchronization for tests that temporarily change process environment.

use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) fn environment_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
