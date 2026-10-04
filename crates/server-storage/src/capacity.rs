use std::path::Path;

use crate::model::StorageCapacity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CapacityError {
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CapacityReport {
    pub capacity: StorageCapacity,
    pub read_only: bool,
}

pub(super) fn inspect_capacity(path: &Path) -> Result<CapacityReport, CapacityError> {
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| CapacityError::Unknown)?;
        // SAFETY: `path` is a NUL-terminated path, and `statistics` points to
        // initialized writable storage for the duration of the call.
        let mut statistics: libc::statvfs = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::statvfs(path.as_ptr(), &mut statistics) };
        if result != 0 {
            return Err(CapacityError::Unknown);
        }
        let read_only = statistics.f_flag & libc::ST_RDONLY != 0;
        let fragment_size = if statistics.f_frsize == 0 {
            statistics.f_bsize as u64
        } else {
            statistics.f_frsize as u64
        };
        let total_bytes = checked_bytes(statistics.f_blocks as u64, fragment_size)
            .ok_or(CapacityError::Unknown)?;
        let available_bytes = checked_bytes(statistics.f_bavail as u64, fragment_size)
            .ok_or(CapacityError::Unknown)?;
        let available_files = to_u64(statistics.f_favail);
        Ok(CapacityReport {
            capacity: StorageCapacity {
                total_bytes,
                available_bytes,
                available_files,
            },
            read_only,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(CapacityError::Unknown)
    }
}

pub(super) fn checked_bytes(blocks: u64, fragment_size: u64) -> Option<u64> {
    blocks.checked_mul(fragment_size)
}

fn to_u64(value: impl TryInto<u64>) -> Option<u64> {
    value.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_capacity_math_is_checked_for_large_values() {
        assert_eq!(checked_bytes(3, 4096), Some(12_288));
        assert_eq!(checked_bytes(u64::MAX, 2), None);
        assert_eq!(checked_bytes(u64::MAX, 1), Some(u64::MAX));
    }

    #[test]
    fn file_capacity_conversion_is_portable_across_linux_integer_widths() {
        assert_eq!(to_u64(7_u32), Some(7));
        assert_eq!(to_u64(7_u64), Some(7));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_capacity_uses_the_filesystem_not_directory_contents() {
        let temp = tempfile::tempdir().unwrap();
        let report = inspect_capacity(temp.path()).expect("statvfs capacity");
        assert!(report.capacity.total_bytes > 0);
        assert!(report.capacity.available_bytes <= report.capacity.total_bytes);
        assert!(report.capacity.available_files.is_some());
    }
}
