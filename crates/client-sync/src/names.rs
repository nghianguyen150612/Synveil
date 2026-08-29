use unicode_normalization::UnicodeNormalization;

/// Result of applying the conservative Windows+Linux segment policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamePortability {
    Portable,
    EmptyOrDot,
    TooLong,
    ContainsSeparator,
    ContainsWindowsIllegalCharacter,
    ContainsControl,
    TrailingSpaceOrDot,
    WindowsReservedDeviceName,
    SynveilControlName,
}

impl NamePortability {
    #[must_use]
    pub const fn is_portable(self) -> bool {
        matches!(self, Self::Portable)
    }
}

/// Validate one exact logical name as a local path segment on both supported
/// desktop families. Portable names are used byte-for-byte; failures are
/// blocked instead of normalized or encoded lossily.
#[must_use]
pub fn validate_logical_name(value: &str) -> NamePortability {
    if value.is_empty() || matches!(value, "." | "..") {
        return NamePortability::EmptyOrDot;
    }
    if value.eq_ignore_ascii_case(".synveil") {
        return NamePortability::SynveilControlName;
    }
    if value.len() > 255 || value.encode_utf16().count() > 255 {
        return NamePortability::TooLong;
    }
    if value.contains(['/', '\\']) {
        return NamePortability::ContainsSeparator;
    }
    if value.chars().any(char::is_control) {
        return NamePortability::ContainsControl;
    }
    if value.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return NamePortability::ContainsWindowsIllegalCharacter;
    }
    if value.ends_with([' ', '.']) {
        return NamePortability::TrailingSpaceOrDot;
    }
    if is_windows_reserved(value) {
        return NamePortability::WindowsReservedDeviceName;
    }
    NamePortability::Portable
}

/// Conservative comparison key used even on case-sensitive hosts so a replica
/// remains movable to Windows. Exact names are still materialized unchanged.
#[must_use]
pub fn local_collision_key(value: &str) -> String {
    value.nfkc().flat_map(char::to_lowercase).collect()
}

fn is_windows_reserved(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    let folded: String = stem.chars().flat_map(char::to_uppercase).collect();
    matches!(
        folded.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || reserved_numbered(&folded, "COM")
        || reserved_numbered(&folded, "LPT")
}

fn reserved_numbered(value: &str, prefix: &str) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return false;
    };
    matches!(
        suffix,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}

#[cfg(test)]
mod tests {
    use super::{NamePortability, local_collision_key, validate_logical_name};

    #[test]
    fn portability_blocks_windows_and_separator_hazards_without_rewriting() {
        assert_eq!(
            validate_logical_name("report.txt"),
            NamePortability::Portable
        );
        assert_eq!(
            validate_logical_name("CON.txt"),
            NamePortability::WindowsReservedDeviceName
        );
        assert_eq!(
            validate_logical_name("folder/file"),
            NamePortability::ContainsSeparator
        );
        assert_eq!(
            validate_logical_name("trailing."),
            NamePortability::TrailingSpaceOrDot
        );
        assert_eq!(
            validate_logical_name(&"a".repeat(256)),
            NamePortability::TooLong
        );
    }

    #[test]
    fn collision_key_is_case_and_normalization_conservative() {
        assert_eq!(
            local_collision_key("Résumé"),
            local_collision_key("RE\u{301}SUME\u{301}")
        );
        assert_eq!(local_collision_key("File"), local_collision_key("file"));
    }
}
