/// Bounded database readiness evidence. It contains no connection details or
/// provider diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseReadiness {
    database_reachable: bool,
    migrations_current: bool,
}

impl DatabaseReadiness {
    #[must_use]
    pub(crate) const fn new(database_reachable: bool, migrations_current: bool) -> Self {
        Self {
            database_reachable,
            migrations_current,
        }
    }

    #[must_use]
    pub const fn database_reachable(self) -> bool {
        self.database_reachable
    }

    #[must_use]
    pub const fn migrations_current(self) -> bool {
        self.migrations_current
    }

    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.database_reachable && self.migrations_current
    }
}

#[cfg(test)]
mod tests {
    use super::DatabaseReadiness;

    #[test]
    fn readiness_requires_reachability_and_current_migrations() {
        assert!(DatabaseReadiness::new(true, true).is_ready());
        assert!(!DatabaseReadiness::new(false, true).is_ready());
        assert!(!DatabaseReadiness::new(true, false).is_ready());
    }
}
