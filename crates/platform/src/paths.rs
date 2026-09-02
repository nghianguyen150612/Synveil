use std::{
    env, fmt,
    path::{Component, Path, PathBuf},
};

use synveil_core::{CacheDir, ConfigDir, DataDir, RuntimeDir};

/// The four standard path roles owned by a platform adapter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PathKind {
    Data,
    Config,
    Cache,
    Runtime,
}

impl PathKind {
    pub const ALL: [Self; 4] = [Self::Data, Self::Config, Self::Cache, Self::Runtime];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Config => "config",
            Self::Cache => "cache",
            Self::Runtime => "runtime",
        }
    }
}

impl fmt::Display for PathKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A resolved set of platform-owned roots.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlatformPaths {
    data_dir: DataDir,
    config_dir: ConfigDir,
    cache_dir: CacheDir,
    runtime_dir: RuntimeDir,
}

impl PlatformPaths {
    #[must_use]
    pub fn new(
        data_dir: impl Into<DataDir>,
        config_dir: impl Into<ConfigDir>,
        cache_dir: impl Into<CacheDir>,
        runtime_dir: impl Into<RuntimeDir>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            config_dir: config_dir.into(),
            cache_dir: cache_dir.into(),
            runtime_dir: runtime_dir.into(),
        }
    }

    #[must_use]
    pub const fn data_dir(&self) -> &DataDir {
        &self.data_dir
    }

    #[must_use]
    pub const fn config_dir(&self) -> &ConfigDir {
        &self.config_dir
    }

    #[must_use]
    pub const fn cache_dir(&self) -> &CacheDir {
        &self.cache_dir
    }

    #[must_use]
    pub const fn runtime_dir(&self) -> &RuntimeDir {
        &self.runtime_dir
    }

    #[must_use]
    pub fn root(&self, kind: PathKind) -> &Path {
        match kind {
            PathKind::Data => self.data_dir.as_path(),
            PathKind::Config => self.config_dir.as_path(),
            PathKind::Cache => self.cache_dir.as_path(),
            PathKind::Runtime => self.runtime_dir.as_path(),
        }
    }

    /// Resolve a relative child without allowing an absolute path or parent
    /// traversal to escape the selected platform root.
    pub fn resolve(
        &self,
        kind: PathKind,
        relative: impl AsRef<Path>,
    ) -> Result<PathBuf, PathResolutionError> {
        let relative = relative.as_ref();

        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
        {
            return Err(PathResolutionError::InvalidRelativePath {
                kind,
                violation: PathResolutionViolation::Absolute,
            });
        }

        if relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(PathResolutionError::InvalidRelativePath {
                kind,
                violation: PathResolutionViolation::ParentTraversal,
            });
        }

        Ok(self.root(kind).join(relative))
    }
}

/// Why a platform path could not be resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathResolutionError {
    MissingEnvironment {
        variable: &'static str,
    },
    InvalidRelativePath {
        kind: PathKind,
        violation: PathResolutionViolation,
    },
}

impl fmt::Display for PathResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment { variable } => {
                write!(
                    formatter,
                    "required platform environment variable {variable} is missing"
                )
            }
            Self::InvalidRelativePath { kind, violation } => {
                write!(formatter, "invalid {kind} relative path: {violation}")
            }
        }
    }
}

impl std::error::Error for PathResolutionError {}

/// A safe-path violation that does not echo an untrusted path value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathResolutionViolation {
    Absolute,
    ParentTraversal,
}

impl fmt::Display for PathResolutionViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Absolute => "absolute paths are not allowed",
            Self::ParentTraversal => "parent traversal is not allowed",
        })
    }
}

/// A path-resolution port implemented by platform adapters.
pub trait PathResolver: Send + Sync {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError>;

    fn resolve_relative(
        &self,
        kind: PathKind,
        relative: &Path,
    ) -> Result<PathBuf, PathResolutionError> {
        self.resolve_paths()?.resolve(kind, relative)
    }
}

/// Compatibility name for callers that use the longer contract name.
pub use PathResolver as PlatformPathResolver;

/// A deterministic resolver useful for tests and higher-level configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedPathResolver {
    paths: PlatformPaths,
}

impl FixedPathResolver {
    #[must_use]
    pub fn new(paths: PlatformPaths) -> Self {
        Self { paths }
    }

    #[must_use]
    pub const fn paths(&self) -> &PlatformPaths {
        &self.paths
    }
}

impl PathResolver for FixedPathResolver {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError> {
        Ok(self.paths.clone())
    }
}

fn environment_path(variable: &'static str) -> Result<PathBuf, PathResolutionError> {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(PathResolutionError::MissingEnvironment { variable })
}

fn environment_path_or(variable: &'static str, fallback: PathBuf) -> PathBuf {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}

fn home_directory() -> Result<PathBuf, PathResolutionError> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .ok_or(PathResolutionError::MissingEnvironment { variable: "HOME" })
}

fn platform_paths(
    data: PathBuf,
    config: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
) -> PlatformPaths {
    PlatformPaths::new(data, config, cache, runtime)
}

pub(crate) fn resolve_generic_paths() -> Result<PlatformPaths, PathResolutionError> {
    Ok(platform_paths(
        environment_path("SYNVEIL_DATA_DIR")?,
        environment_path("SYNVEIL_CONFIG_DIR")?,
        environment_path("SYNVEIL_CACHE_DIR")?,
        environment_path("SYNVEIL_RUNTIME_DIR")?,
    ))
}

pub(crate) fn resolve_linux_paths() -> Result<PlatformPaths, PathResolutionError> {
    let home = home_directory()?;
    let data_base = environment_path_or("XDG_DATA_HOME", home.join(".local/share"));
    let config_base = environment_path_or("XDG_CONFIG_HOME", home.join(".config"));
    let cache_base = environment_path_or("XDG_CACHE_HOME", home.join(".cache"));
    let state_base = environment_path_or("XDG_STATE_HOME", home.join(".local/state"));
    let application = "synveil";

    Ok(platform_paths(
        environment_path_or("SYNVEIL_DATA_DIR", data_base.join(application)),
        environment_path_or("SYNVEIL_CONFIG_DIR", config_base.join(application)),
        environment_path_or("SYNVEIL_CACHE_DIR", cache_base.join(application)),
        environment_path_or(
            "SYNVEIL_RUNTIME_DIR",
            env::var_os("XDG_RUNTIME_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| state_base.join(application).join("runtime")),
        ),
    ))
}

pub(crate) fn resolve_windows_paths() -> Result<PlatformPaths, PathResolutionError> {
    let home = home_directory()?;
    let roaming = environment_path_or("APPDATA", home.join("AppData/Roaming"));
    let local = environment_path_or("LOCALAPPDATA", home.join("AppData/Local"));
    let application = "Synveil";

    Ok(platform_paths(
        environment_path_or("SYNVEIL_DATA_DIR", local.join(application)),
        environment_path_or("SYNVEIL_CONFIG_DIR", roaming.join(application)),
        environment_path_or("SYNVEIL_CACHE_DIR", local.join(application).join("Cache")),
        environment_path_or(
            "SYNVEIL_RUNTIME_DIR",
            local.join(application).join("Runtime"),
        ),
    ))
}

pub(crate) fn resolve_macos_paths() -> Result<PlatformPaths, PathResolutionError> {
    let home = home_directory()?;
    let application_support = home.join("Library/Application Support/Synveil");
    let preferences = home.join("Library/Preferences/Synveil");
    let caches = home.join("Library/Caches/Synveil");

    Ok(platform_paths(
        environment_path_or("SYNVEIL_DATA_DIR", application_support.clone()),
        environment_path_or("SYNVEIL_CONFIG_DIR", preferences),
        environment_path_or("SYNVEIL_CACHE_DIR", caches),
        environment_path_or("SYNVEIL_RUNTIME_DIR", application_support.join("Runtime")),
    ))
}
