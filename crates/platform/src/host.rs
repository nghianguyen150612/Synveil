use std::fmt;

/// The host families for which Synveil has an explicit adapter namespace.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Platform {
    Linux,
    Windows,
    Macos,
    /// Portable fallback for an unrecognised host or the generic adapter.
    #[default]
    Generic,
}

impl Platform {
    /// Associated-constant spelling used by some platform APIs.
    #[allow(non_upper_case_globals)]
    pub const MacOS: Self = Self::Macos;

    /// Associated-constant spelling used by some platform APIs.
    #[allow(non_upper_case_globals)]
    pub const MacOs: Self = Self::Macos;

    #[must_use]
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "macos" => Self::Macos,
            _ => Self::Generic,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Generic => "generic",
        }
    }

    #[must_use]
    pub const fn is_primary_desktop(self) -> bool {
        matches!(self, Self::Linux | Self::Windows | Self::Macos)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A coarse host architecture used for diagnostics and adapter selection.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HostArchitecture {
    X86,
    X86_64,
    Arm,
    Aarch64,
    Wasm,
    #[default]
    Other,
}

impl HostArchitecture {
    #[must_use]
    pub fn detect() -> Self {
        match std::env::consts::ARCH {
            "x86" => Self::X86,
            "x86_64" => Self::X86_64,
            "arm" => Self::Arm,
            "aarch64" => Self::Aarch64,
            "wasm32" | "wasm64" => Self::Wasm,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
            Self::Arm => "arm",
            Self::Aarch64 => "aarch64",
            Self::Wasm => "wasm",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for HostArchitecture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Non-sensitive host facts that are safe for a platform adapter to expose.
///
/// Hostnames, usernames, mount paths, and OS-specific diagnostic blobs are
/// intentionally not part of this baseline contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostInfo {
    platform: Platform,
    architecture: HostArchitecture,
    target_os: &'static str,
    target_architecture: &'static str,
}

impl HostInfo {
    #[must_use]
    pub fn current() -> Self {
        Self::for_platform(Platform::detect())
    }

    #[must_use]
    pub fn for_platform(platform: Platform) -> Self {
        Self {
            platform,
            architecture: HostArchitecture::detect(),
            target_os: std::env::consts::OS,
            target_architecture: std::env::consts::ARCH,
        }
    }

    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    #[must_use]
    pub const fn architecture(&self) -> HostArchitecture {
        self.architecture
    }

    #[must_use]
    pub const fn target_os(&self) -> &'static str {
        self.target_os
    }

    #[must_use]
    pub const fn target_architecture(&self) -> &'static str {
        self.target_architecture
    }
}
