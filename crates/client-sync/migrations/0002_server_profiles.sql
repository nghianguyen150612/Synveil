-- Forward-only desktop connection metadata. Secret material belongs exclusively
-- to PlatformRuntime::SecretStore and is never a column in this database.
CREATE TABLE server_profiles (
    profile_id TEXT PRIMARY KEY CHECK(length(profile_id) = 36),
    canonical_base_url TEXT NOT NULL CHECK(length(canonical_base_url) BETWEEN 1 AND 2048),
    transport_policy TEXT NOT NULL CHECK(transport_policy IN ('HTTPS', 'LOOPBACK_TEST_HTTP')),
    display_label TEXT NOT NULL CHECK(length(CAST(display_label AS BLOB)) BETWEEN 1 AND 256),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    last_connected_at_ms INTEGER CHECK(last_connected_at_ms IS NULL OR last_connected_at_ms >= created_at_ms)
) STRICT;

CREATE TRIGGER server_profile_identity_is_immutable
BEFORE UPDATE OF profile_id, canonical_base_url, transport_policy ON server_profiles
WHEN NEW.profile_id != OLD.profile_id
  OR NEW.canonical_base_url != OLD.canonical_base_url
  OR NEW.transport_policy != OLD.transport_policy
BEGIN
    SELECT RAISE(ABORT, 'server profile identity is immutable');
END;

CREATE TABLE profile_device_enrollments (
    profile_id TEXT PRIMARY KEY REFERENCES server_profiles(profile_id),
    owner_user_id TEXT NOT NULL CHECK(length(owner_user_id) = 36),
    device_id TEXT NOT NULL CHECK(length(device_id) = 36),
    credential_id TEXT NOT NULL CHECK(length(credential_id) = 36),
    completed_at_ms INTEGER NOT NULL CHECK(completed_at_ms >= 0),
    forgotten_at_ms INTEGER CHECK(forgotten_at_ms IS NULL OR forgotten_at_ms >= completed_at_ms)
) STRICT;

CREATE TRIGGER profile_enrollment_scope_is_immutable
BEFORE UPDATE OF profile_id, owner_user_id, device_id ON profile_device_enrollments
WHEN NEW.profile_id != OLD.profile_id
  OR NEW.owner_user_id != OLD.owner_user_id
  OR NEW.device_id != OLD.device_id
BEGIN
    SELECT RAISE(ABORT, 'profile enrollment scope is immutable');
END;

-- Write a non-secret intent before touching the external secret store. This
-- also makes replacement/forget cleanup retryable after interruption.
CREATE TABLE profile_secret_cleanup (
    profile_id TEXT NOT NULL REFERENCES server_profiles(profile_id),
    credential_id TEXT NOT NULL CHECK(length(credential_id) = 36),
    requested_at_ms INTEGER NOT NULL CHECK(requested_at_ms >= 0),
    PRIMARY KEY(profile_id, credential_id)
) STRICT;

ALTER TABLE replicas ADD COLUMN server_profile_id TEXT REFERENCES server_profiles(profile_id)
    CHECK(server_profile_id IS NULL OR length(server_profile_id) = 36);

-- Legacy V1 replicas remain explicitly unbound. No automatic inference from
-- their LibraryId, DeviceId, or hostname can turn them into a production replica.
CREATE TRIGGER replica_server_profile_is_immutable
BEFORE UPDATE OF server_profile_id ON replicas
WHEN NEW.server_profile_id IS NOT OLD.server_profile_id
BEGIN
    SELECT RAISE(ABORT, 'replica server profile is immutable');
END;
