use std::{fmt, str::FromStr};

use uuid::Uuid;

/// The error returned when a public domain ID is not a canonical UUIDv7.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdParseError {
    InvalidUuid,
    NonCanonical,
    InvalidVariant,
    NotUuidV7,
}

impl fmt::Display for IdParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidUuid => "invalid UUID",
            Self::NonCanonical => "UUID is not in canonical lowercase hyphenated form",
            Self::InvalidVariant => "UUID has an unsupported variant",
            Self::NotUuidV7 => "UUID is not version 7",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for IdParseError {}

fn validate_uuid(value: Uuid) -> Result<Uuid, IdParseError> {
    let bytes = value.as_bytes();

    if bytes[6] >> 4 != 7 {
        return Err(IdParseError::NotUuidV7);
    }

    // RFC 9562's UUID variant is binary 10xxxxxx.
    if bytes[8] & 0xc0 != 0x80 {
        return Err(IdParseError::InvalidVariant);
    }

    Ok(value)
}

fn parse_uuid(value: &str) -> Result<Uuid, IdParseError> {
    let uuid = Uuid::parse_str(value).map_err(|_| IdParseError::InvalidUuid)?;

    if value != uuid.hyphenated().to_string() {
        return Err(IdParseError::NonCanonical);
    }

    validate_uuid(uuid)
}

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new opaque UUIDv7 domain identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Construct an ID from a UUID after validating its public form.
            pub fn try_from_uuid(value: Uuid) -> Result<Self, IdParseError> {
                validate_uuid(value).map(Self)
            }

            pub fn parse_str(value: &str) -> Result<Self, IdParseError> {
                value.parse()
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            #[must_use]
            pub const fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl AsRef<Uuid> for $name {
            fn as_ref(&self) -> &Uuid {
                self.as_uuid()
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.into_uuid()
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_uuid(value).map(Self)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdParseError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                value.parse()
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = IdParseError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::try_from_uuid(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0.hyphenated())
            }
        }
    };
}

domain_id!(UserId);
domain_id!(DeviceId);
domain_id!(DeviceCredentialId);
domain_id!(DeviceEnrollmentGrantId);
domain_id!(LibraryId);
domain_id!(DedupDomainId);
domain_id!(NodeId);
domain_id!(FileVersionId);
domain_id!(ObjectId);
domain_id!(ObjectReplicaId);
domain_id!(ObjectGcOperationId);
domain_id!(UploadSessionId);
domain_id!(BackupSetId);
domain_id!(SnapshotId);
domain_id!(BackupPrunePlanId);
domain_id!(BackupPruneExecutionId);
domain_id!(BackupRestorePlanId);
domain_id!(BackupRestoreExecutionId);
domain_id!(BackupSnapshotRetentionPolicyRevisionId);
domain_id!(BackupSnapshotExpiryPlanId);
domain_id!(BackupSnapshotExpiryExecutionId);
domain_id!(BackupMaintenanceRunId);
domain_id!(ShareId);
domain_id!(ChangeEventId);
domain_id!(SyncBootstrapId);
domain_id!(ClientMutationId);
// A local control-plane ID. This is deliberately distinct from the server
// ClientMutationId used only after an explicit future submission phase.
domain_id!(OutboundIntentId);
domain_id!(SyncConflictId);
domain_id!(ConflictResolutionId);

#[cfg(test)]
mod tests {
    use std::{any::TypeId, str::FromStr};

    use super::{
        ClientMutationId, ConflictResolutionId, DeviceCredentialId, DeviceEnrollmentGrantId,
        IdParseError, NodeId, OutboundIntentId, SyncConflictId, UserId,
    };
    use uuid::Uuid;

    #[test]
    fn generated_ids_are_canonical_uuidv7_values() {
        let id = UserId::new();
        let serialized = id.to_string();

        assert_eq!(serialized.len(), 36);
        assert_eq!(serialized, serialized.to_ascii_lowercase());
        assert_eq!(UserId::from_str(&serialized), Ok(id));
        assert_eq!(id.as_uuid().as_bytes()[6] >> 4, 7);
        assert_eq!(id.as_uuid().as_bytes()[8] & 0xc0, 0x80);
    }

    #[test]
    fn credential_and_grant_ids_are_distinct_nonsecret_uuidv7_identities() {
        let credential = DeviceCredentialId::new();
        let grant = DeviceEnrollmentGrantId::new();
        assert_eq!(
            DeviceCredentialId::parse_str(&credential.to_string()),
            Ok(credential)
        );
        assert_eq!(
            DeviceEnrollmentGrantId::parse_str(&grant.to_string()),
            Ok(grant)
        );
        assert_eq!(
            DeviceCredentialId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
        assert_eq!(
            DeviceEnrollmentGrantId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
        assert_ne!(
            TypeId::of::<DeviceCredentialId>(),
            TypeId::of::<DeviceEnrollmentGrantId>()
        );
    }

    #[test]
    fn ids_reject_noncanonical_or_non_v7_values() {
        let id = UserId::new().to_string();
        assert_eq!(
            UserId::from_str(&id.replace('-', "")),
            Err(IdParseError::NonCanonical)
        );
        assert_eq!(
            UserId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
    }

    #[test]
    fn domain_id_types_are_distinct() {
        assert_ne!(TypeId::of::<UserId>(), TypeId::of::<NodeId>());
        assert_ne!(TypeId::of::<ClientMutationId>(), TypeId::of::<NodeId>());
    }

    #[test]
    fn client_mutation_ids_use_the_same_canonical_uuidv7_boundary() {
        let id = ClientMutationId::new();
        assert_eq!(ClientMutationId::from_str(&id.to_string()), Ok(id));
        assert_eq!(
            ClientMutationId::from_str(&id.to_string().to_ascii_uppercase()),
            Err(IdParseError::NonCanonical)
        );
        assert_eq!(
            ClientMutationId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
    }

    #[test]
    fn outbound_intent_ids_are_local_uuidv7_values_not_server_mutation_ids() {
        let intent = OutboundIntentId::new();
        assert_eq!(OutboundIntentId::from_str(&intent.to_string()), Ok(intent));
        assert_eq!(
            OutboundIntentId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
        assert_ne!(
            TypeId::of::<OutboundIntentId>(),
            TypeId::of::<ClientMutationId>()
        );
    }

    #[test]
    fn conflict_and_resolution_ids_use_the_canonical_uuidv7_boundary() {
        let conflict_id = SyncConflictId::new();
        let resolution_id = ConflictResolutionId::new();

        assert_eq!(
            SyncConflictId::from_str(&conflict_id.to_string()),
            Ok(conflict_id)
        );
        assert_eq!(
            ConflictResolutionId::from_str(&resolution_id.to_string()),
            Ok(resolution_id)
        );
        assert_eq!(
            SyncConflictId::try_from_uuid(Uuid::nil()),
            Err(IdParseError::NotUuidV7)
        );
        assert_ne!(
            TypeId::of::<SyncConflictId>(),
            TypeId::of::<ConflictResolutionId>()
        );
    }
}
