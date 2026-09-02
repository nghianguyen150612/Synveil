use std::{
    fmt,
    path::{Path, PathBuf},
};

macro_rules! logical_path {
    ($name:ident) => {
        /// A logical path supplied by configuration; platform resolution is external to core.
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(PathBuf);

        impl $name {
            #[must_use]
            pub fn new(path: impl Into<PathBuf>) -> Self {
                Self(path.into())
            }

            #[must_use]
            pub fn as_path(&self) -> &Path {
                &self.0
            }

            #[must_use]
            pub fn into_path_buf(self) -> PathBuf {
                self.0
            }
        }

        impl From<PathBuf> for $name {
            fn from(path: PathBuf) -> Self {
                Self::new(path)
            }
        }

        impl AsRef<Path> for $name {
            fn as_ref(&self) -> &Path {
                self.as_path()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.display().fmt(formatter)
            }
        }
    };
}

logical_path!(DataDir);
logical_path!(ConfigDir);
logical_path!(CacheDir);
logical_path!(RuntimeDir);

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{CacheDir, ConfigDir, DataDir, RuntimeDir};

    #[test]
    fn logical_paths_preserve_portable_path_values() {
        let path = PathBuf::from("synveil").join("data");

        assert_eq!(DataDir::new(path.clone()).as_path(), path.as_path());
        assert_eq!(ConfigDir::from(path.clone()).into_path_buf(), path);
        assert_eq!(CacheDir::new("cache").to_string(), "cache");
        assert_eq!(
            RuntimeDir::new("runtime").as_ref(),
            std::path::Path::new("runtime")
        );
    }
}
