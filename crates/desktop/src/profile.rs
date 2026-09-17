//! Controller-boundary profile identity helpers for the native shell.
//!
//! Manifest path resolution and profile parsing stay inside `synveil-client`.
//! This module keeps the focused shell tests close to the bridge without
//! adding a direct platform or filesystem configuration dependency.

use synveil_client::{DesktopClientConfig, DesktopClientConfigError, ServerProfileId};

pub type ProfileLoadError = DesktopClientConfigError;

pub fn load_profile_id() -> Result<ServerProfileId, ProfileLoadError> {
    DesktopClientConfig::configured_profile_id()
}

#[cfg(test)]
pub fn parse_profile_id(text: &str) -> Result<ServerProfileId, ProfileLoadError> {
    DesktopClientConfig::parse_profile_id(text)
}

#[cfg(test)]
mod tests {
    use super::{parse_profile_id, ProfileLoadError};
    use synveil_client::ServerProfileId;

    #[test]
    fn reads_only_profile_id_and_ignores_root_value() {
        let profile_id = ServerProfileId::new();
        let text = format!(
            "# non-secret manifest\nprofile_id={profile_id}\nlibrary.01900000-0000-7000-8000-000000000001=/private/root\n"
        );
        assert_eq!(
            parse_profile_id(&text).expect("profile identity"),
            profile_id
        );
    }

    #[test]
    fn rejects_duplicate_or_missing_identity() {
        let profile_id = ServerProfileId::new();
        assert!(matches!(
            parse_profile_id(&format!("profile_id={profile_id}\nprofile_id={profile_id}")),
            Err(ProfileLoadError::DuplicateProfile)
        ));
        assert!(matches!(
            parse_profile_id("library.01900000-0000-7000-8000-000000000001=/root"),
            Err(ProfileLoadError::MissingProfileId)
        ));
    }

    #[test]
    fn rejects_unknown_keys_without_echoing_values() {
        assert!(matches!(
            parse_profile_id("profile_id=not-a-profile\nsecret=do-not-echo"),
            Err(ProfileLoadError::InvalidProfileId)
        ));
        assert!(matches!(
            parse_profile_id("profile_id=01900000-0000-7000-8000-000000000001\nother=value"),
            Err(ProfileLoadError::UnknownKey)
        ));
    }
}
