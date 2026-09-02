use std::{fmt, str::FromStr};

/// The operation whose destination intent is frozen when an upload session is
/// created.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UploadOperation {
    CreateFile,
    ReplaceContent,
}

impl UploadOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateFile => "CREATE_FILE",
            Self::ReplaceContent => "REPLACE_CONTENT",
        }
    }
}

impl fmt::Display for UploadOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadOperationParseError;

impl fmt::Display for UploadOperationParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("upload operation is unknown")
    }
}

impl std::error::Error for UploadOperationParseError {}

impl FromStr for UploadOperation {
    type Err = UploadOperationParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATE_FILE" => Ok(Self::CreateFile),
            "REPLACE_CONTENT" => Ok(Self::ReplaceContent),
            _ => Err(UploadOperationParseError),
        }
    }
}

/// Publicly persisted upload-session states. Internal verifier phases remain
/// diagnostics/recovery data inside `VERIFYING`; they are not additional
/// states that callers may transition into.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UploadSessionState {
    Open,
    Verifying,
    Committing,
    Committed,
    Failed,
    Expired,
    Aborted,
}

impl UploadSessionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Verifying => "VERIFYING",
            Self::Committing => "COMMITTING",
            Self::Committed => "COMMITTED",
            Self::Failed => "FAILED",
            Self::Expired => "EXPIRED",
            Self::Aborted => "ABORTED",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Committed | Self::Failed | Self::Expired | Self::Aborted
        )
    }

    /// Validate a durable state transition in one place. Repeating the same
    /// state is allowed for idempotent recovery updates; reversing a terminal
    /// or pre-commit state is never allowed.
    pub fn transition(self, next: Self) -> Result<Self, UploadStateTransitionError> {
        let allowed = self == next
            || matches!(
                (self, next),
                (Self::Open, Self::Verifying | Self::Aborted | Self::Expired)
                    | (
                        Self::Verifying,
                        Self::Committing | Self::Aborted | Self::Failed
                    )
                    | (
                        Self::Committing,
                        Self::Committed | Self::Aborted | Self::Failed
                    )
            );

        if allowed {
            Ok(next)
        } else {
            Err(UploadStateTransitionError {
                from: self,
                to: next,
            })
        }
    }
}

impl fmt::Display for UploadSessionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadSessionStateParseError;

impl fmt::Display for UploadSessionStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("upload session state is unknown")
    }
}

impl std::error::Error for UploadSessionStateParseError {}

impl FromStr for UploadSessionState {
    type Err = UploadSessionStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "OPEN" => Ok(Self::Open),
            "VERIFYING" => Ok(Self::Verifying),
            "COMMITTING" => Ok(Self::Committing),
            "COMMITTED" => Ok(Self::Committed),
            "FAILED" => Ok(Self::Failed),
            "EXPIRED" => Ok(Self::Expired),
            "ABORTED" => Ok(Self::Aborted),
            _ => Err(UploadSessionStateParseError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadStateTransitionError {
    from: UploadSessionState,
    to: UploadSessionState,
}

impl UploadStateTransitionError {
    #[must_use]
    pub const fn from(self) -> UploadSessionState {
        self.from
    }

    #[must_use]
    pub const fn to(self) -> UploadSessionState {
        self.to
    }
}

impl fmt::Display for UploadStateTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "upload state transition {} -> {} is not allowed",
            self.from, self.to
        )
    }
}

impl std::error::Error for UploadStateTransitionError {}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{UploadOperation, UploadSessionState};

    #[test]
    fn upload_states_allow_only_forward_or_terminal_transitions() {
        assert_eq!(
            UploadSessionState::Open
                .transition(UploadSessionState::Verifying)
                .unwrap(),
            UploadSessionState::Verifying
        );
        assert!(
            UploadSessionState::Committed
                .transition(UploadSessionState::Open)
                .is_err()
        );
        assert!(
            UploadSessionState::Verifying
                .transition(UploadSessionState::Open)
                .is_err()
        );
        assert!(
            UploadSessionState::Aborted
                .transition(UploadSessionState::Aborted)
                .is_ok()
        );
    }

    #[test]
    fn upload_contract_values_round_trip() {
        assert_eq!(UploadOperation::CreateFile.as_str(), "CREATE_FILE");
        assert_eq!(
            UploadOperation::from_str("REPLACE_CONTENT"),
            Ok(UploadOperation::ReplaceContent)
        );
        assert_eq!(
            UploadSessionState::from_str("COMMITTING"),
            Ok(UploadSessionState::Committing)
        );
    }
}
