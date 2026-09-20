-- Prompt 102: a desktop user may correct the non-secret server origin or
-- display label while the opaque profile identity remains immutable.
-- Credential fencing/SecretStore cleanup is performed by the canonical Rust
-- lifecycle before the URL update is committed.
DROP TRIGGER server_profile_identity_is_immutable;

CREATE TRIGGER server_profile_id_is_immutable
BEFORE UPDATE OF profile_id ON server_profiles
WHEN NEW.profile_id != OLD.profile_id
BEGIN
    SELECT RAISE(ABORT, 'server profile id is immutable');
END;
