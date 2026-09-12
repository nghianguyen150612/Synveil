//! Explicit conversion between PostgreSQL row values and canonical domain
//! values.

use std::{fmt, str::FromStr};

use synveil_core::{
    Device, DeviceId, DeviceStatus, DomainError, FileVersion, FileVersionId, Library, LibraryId,
    LibraryStatus, LogicalName, LoginIdentifier, Node, NodeId, NodeKind, NodeState,
    ObjectReference, Revision, Sequence, Sha256Digest, Timestamp, User, UserId, UserStatus,
};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::models::{DeviceRow, FileVersionRow, LibraryRow, NodeRow, ObjectRow, UserRow};

/// Safe failures produced while decoding or encoding a persisted row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    InvalidId {
        field: &'static str,
        reason: synveil_core::IdParseError,
    },
    InvalidTimestamp {
        field: &'static str,
    },
    TimestampPrecisionLoss {
        field: &'static str,
    },
    InvalidDecimal {
        field: &'static str,
    },
    InvalidDigest {
        field: &'static str,
    },
    InvalidEnum {
        field: &'static str,
    },
    InvalidName {
        field: &'static str,
    },
    InvalidLogin {
        field: &'static str,
    },
    RelationMismatch {
        relation: &'static str,
    },
    RevisionConflict {
        current_revision: Revision,
    },
    Domain(DomainError),
}

impl fmt::Display for MappingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { field, .. } => {
                write!(formatter, "persisted {field} is not a canonical UUIDv7")
            }
            Self::InvalidTimestamp { field } => {
                write!(formatter, "persisted {field} is not a valid UTC timestamp")
            }
            Self::TimestampPrecisionLoss { field } => {
                write!(
                    formatter,
                    "persisted {field} exceeds PostgreSQL timestamp precision"
                )
            }
            Self::InvalidDecimal { field } => {
                write!(
                    formatter,
                    "persisted {field} is not a valid unsigned decimal"
                )
            }
            Self::InvalidDigest { field } => {
                write!(
                    formatter,
                    "persisted {field} is not a 32-byte SHA-256 digest"
                )
            }
            Self::InvalidEnum { field } => write!(formatter, "persisted {field} is unknown"),
            Self::InvalidName { field } => write!(formatter, "persisted {field} is invalid"),
            Self::InvalidLogin { field } => write!(formatter, "persisted {field} is invalid"),
            Self::RelationMismatch { relation } => {
                write!(
                    formatter,
                    "persisted {relation} relationship is inconsistent"
                )
            }
            Self::RevisionConflict { .. } => {
                formatter.write_str("the resource revision changed before the operation")
            }
            Self::Domain(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MappingError {}

impl From<DomainError> for MappingError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

fn encode_timestamp(value: Timestamp, field: &'static str) -> Result<OffsetDateTime, MappingError> {
    let value = value.as_offset_datetime().to_offset(UtcOffset::UTC);
    if !value.nanosecond().is_multiple_of(1_000) {
        return Err(MappingError::TimestampPrecisionLoss { field });
    }
    Ok(value)
}

fn decode_timestamp(value: OffsetDateTime, _field: &'static str) -> Timestamp {
    Timestamp::from_offset_datetime(value)
}

fn encode_revision(value: Revision) -> String {
    value.get().to_string()
}

fn encode_sequence(value: Sequence) -> String {
    value.get().to_string()
}

/// Encode a domain revision for a PostgreSQL `NUMERIC` bind.
#[must_use]
pub fn revision_to_decimal(value: Revision) -> String {
    encode_revision(value)
}

/// Encode a domain sequence for a PostgreSQL `NUMERIC` bind.
#[must_use]
pub fn sequence_to_decimal(value: Sequence) -> String {
    encode_sequence(value)
}

/// Decode a PostgreSQL `NUMERIC::text` revision without narrowing it first.
pub fn revision_from_decimal(value: &str) -> Result<Revision, MappingError> {
    Revision::from_str(value).map_err(|_| MappingError::InvalidDecimal { field: "revision" })
}

/// Decode a PostgreSQL `NUMERIC::text` sequence without narrowing it first.
pub fn sequence_from_decimal(value: &str) -> Result<Sequence, MappingError> {
    Sequence::from_str(value).map_err(|_| MappingError::InvalidDecimal { field: "sequence" })
}

fn encode_id<T>(id: &T) -> Uuid
where
    T: AsRef<Uuid>,
{
    *id.as_ref()
}

macro_rules! decode_id {
    ($value:expr, $type:ty, $field:literal) => {
        <$type>::try_from_uuid($value).map_err(|reason| MappingError::InvalidId {
            field: $field,
            reason,
        })
    };
}

fn encode_name(value: &LogicalName) -> String {
    value.as_str().to_owned()
}

fn decode_name(value: String, field: &'static str) -> Result<LogicalName, MappingError> {
    LogicalName::new(value).map_err(|_| MappingError::InvalidName { field })
}

fn encode_login(value: &LoginIdentifier) -> (String, String) {
    (value.value().to_owned(), value.uniqueness_key().to_owned())
}

fn decode_login(
    value: String,
    uniqueness_key: String,
    field: &'static str,
) -> Result<LoginIdentifier, MappingError> {
    LoginIdentifier::new(value, uniqueness_key).map_err(|_| MappingError::InvalidLogin { field })
}

fn decode_user_status(value: &str) -> Result<UserStatus, MappingError> {
    match value {
        "PENDING" => Ok(UserStatus::Pending),
        "ACTIVE" => Ok(UserStatus::Active),
        "LOCKED" => Ok(UserStatus::Locked),
        "DISABLED" => Ok(UserStatus::Disabled),
        _ => Err(MappingError::InvalidEnum {
            field: "users.status",
        }),
    }
}

fn decode_device_status(value: &str) -> Result<DeviceStatus, MappingError> {
    match value {
        "PENDING" => Ok(DeviceStatus::Pending),
        "ACTIVE" => Ok(DeviceStatus::Active),
        "PAUSED" => Ok(DeviceStatus::Paused),
        "REVOKED" => Ok(DeviceStatus::Revoked),
        _ => Err(MappingError::InvalidEnum {
            field: "devices.status",
        }),
    }
}

fn decode_library_status(value: &str) -> Result<LibraryStatus, MappingError> {
    match value {
        "ACTIVE" => Ok(LibraryStatus::Active),
        "READ_ONLY" => Ok(LibraryStatus::ReadOnly),
        "QUARANTINED" => Ok(LibraryStatus::Quarantined),
        "DELETING" => Ok(LibraryStatus::Deleting),
        _ => Err(MappingError::InvalidEnum {
            field: "libraries.status",
        }),
    }
}

fn decode_node_kind(value: &str) -> Result<NodeKind, MappingError> {
    match value {
        "FILE" => Ok(NodeKind::File),
        "DIRECTORY" => Ok(NodeKind::Directory),
        _ => Err(MappingError::InvalidEnum {
            field: "nodes.kind",
        }),
    }
}

fn decode_node_state(value: &str) -> Result<NodeState, MappingError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        "PURGING" => Ok(NodeState::Purging),
        _ => Err(MappingError::InvalidEnum {
            field: "nodes.state",
        }),
    }
}

fn decode_digest(value: &[u8], field: &'static str) -> Result<Sha256Digest, MappingError> {
    Sha256Digest::try_from(value).map_err(|_| MappingError::InvalidDigest { field })
}

impl UserRow {
    pub fn from_domain(value: &User) -> Result<Self, MappingError> {
        let (login_value, login_key) = encode_login(value.login());
        Ok(Self {
            id: encode_id(&value.id()),
            login_value,
            login_key,
            status: value.status().as_str().to_owned(),
            is_instance_admin: value.is_instance_admin(),
            created_at: encode_timestamp(value.created_at(), "users.created_at")?,
            updated_at: encode_timestamp(value.updated_at(), "users.updated_at")?,
            revision: encode_revision(value.revision()),
        })
    }

    pub fn try_into_domain(self) -> Result<User, MappingError> {
        Ok(User::rehydrate_with_admin(
            decode_id!(self.id, UserId, "users.id")?,
            decode_login(self.login_value, self.login_key, "users.login")?,
            decode_user_status(&self.status)?,
            self.is_instance_admin,
            decode_timestamp(self.created_at, "users.created_at"),
            decode_timestamp(self.updated_at, "users.updated_at"),
            revision_from_field(&self.revision, "users.revision")?,
        ))
    }
}

impl DeviceRow {
    pub fn from_domain(value: &Device) -> Result<Self, MappingError> {
        Ok(Self {
            id: encode_id(&value.id()),
            owner_user_id: encode_id(&value.owner_user_id()),
            display_name: encode_name(value.display_name()),
            status: value.status().as_str().to_owned(),
            created_at: encode_timestamp(value.created_at(), "devices.created_at")?,
            updated_at: encode_timestamp(value.updated_at(), "devices.updated_at")?,
            revision: encode_revision(value.revision()),
        })
    }

    pub fn try_into_domain(self) -> Result<Device, MappingError> {
        Ok(Device::rehydrate(
            decode_id!(self.id, DeviceId, "devices.id")?,
            decode_id!(self.owner_user_id, UserId, "devices.owner_user_id")?,
            decode_name(self.display_name, "devices.display_name")?,
            decode_device_status(&self.status)?,
            decode_timestamp(self.created_at, "devices.created_at"),
            decode_timestamp(self.updated_at, "devices.updated_at"),
            revision_from_field(&self.revision, "devices.revision")?,
        ))
    }
}

impl LibraryRow {
    pub fn from_domain(value: &Library) -> Result<Self, MappingError> {
        Ok(Self {
            id: encode_id(&value.id()),
            owner_user_id: encode_id(&value.owner_user_id()),
            name: encode_name(value.name()),
            root_node_id: encode_id(&value.root_node_id()),
            dedup_domain_id: encode_id(&value.dedup_domain_id()),
            status: value.status().as_str().to_owned(),
            created_at: encode_timestamp(value.created_at(), "libraries.created_at")?,
            updated_at: encode_timestamp(value.updated_at(), "libraries.updated_at")?,
            revision: encode_revision(value.revision()),
        })
    }

    pub fn try_into_domain(self, root: &Node) -> Result<Library, MappingError> {
        let id = decode_id!(self.id, LibraryId, "libraries.id")?;
        let root_node_id = decode_id!(self.root_node_id, NodeId, "libraries.root_node_id")?;
        if root.id() != root_node_id {
            return Err(MappingError::RelationMismatch {
                relation: "libraries.root_node_id",
            });
        }

        Ok(Library::rehydrate(
            id,
            decode_id!(self.owner_user_id, UserId, "libraries.owner_user_id")?,
            decode_name(self.name, "libraries.name")?,
            decode_library_status(&self.status)?,
            root,
            decode_id!(
                self.dedup_domain_id,
                synveil_core::DedupDomainId,
                "libraries.dedup_domain_id"
            )?,
            decode_timestamp(self.created_at, "libraries.created_at"),
            decode_timestamp(self.updated_at, "libraries.updated_at"),
            revision_from_field(&self.revision, "libraries.revision")?,
        )?)
    }
}

impl NodeRow {
    pub fn from_domain(value: &Node) -> Result<Self, MappingError> {
        Ok(Self {
            id: encode_id(&value.id()),
            library_id: encode_id(&value.library_id()),
            parent_node_id: value.parent_node_id().map(|id| encode_id(&id)),
            kind: value.kind().as_str().to_owned(),
            name: encode_name(value.name()),
            current_version_id: value.current_version_id().map(|id| encode_id(&id)),
            state: value.state().as_str().to_owned(),
            trashed_at: value
                .trashed_at()
                .map(|timestamp| encode_timestamp(timestamp, "nodes.trashed_at"))
                .transpose()?,
            created_at: encode_timestamp(value.created_at(), "nodes.created_at")?,
            updated_at: encode_timestamp(value.updated_at(), "nodes.updated_at")?,
            revision: encode_revision(value.revision()),
        })
    }

    pub fn try_into_domain(self) -> Result<Node, MappingError> {
        Ok(Node::rehydrate_with_trash(
            decode_id!(self.id, NodeId, "nodes.id")?,
            decode_id!(self.library_id, LibraryId, "nodes.library_id")?,
            self.parent_node_id
                .map(|id| decode_id!(id, NodeId, "nodes.parent_node_id"))
                .transpose()?,
            decode_node_kind(&self.kind)?,
            decode_name(self.name, "nodes.name")?,
            self.current_version_id
                .map(|id| decode_id!(id, FileVersionId, "nodes.current_version_id"))
                .transpose()?,
            decode_node_state(&self.state)?,
            self.trashed_at
                .map(|timestamp| decode_timestamp(timestamp, "nodes.trashed_at")),
            decode_timestamp(self.created_at, "nodes.created_at"),
            decode_timestamp(self.updated_at, "nodes.updated_at"),
            revision_from_field(&self.revision, "nodes.revision")?,
        )?)
    }
}

impl ObjectRow {
    pub fn from_reference(
        value: ObjectReference,
        created_at: Timestamp,
    ) -> Result<Self, MappingError> {
        Ok(Self {
            id: encode_id(&value.object_id()),
            dedup_domain_id: encode_id(&value.dedup_domain_id()),
            canonical_hash: value.canonical_hash().as_bytes().to_vec(),
            plaintext_length: value.plaintext_length().to_string(),
            created_at: encode_timestamp(created_at, "objects.created_at")?,
        })
    }

    pub fn try_into_reference(&self) -> Result<ObjectReference, MappingError> {
        Ok(ObjectReference::new(
            decode_id!(self.id, synveil_core::ObjectId, "objects.id")?,
            decode_id!(
                self.dedup_domain_id,
                synveil_core::DedupDomainId,
                "objects.dedup_domain_id"
            )?,
            decode_digest(&self.canonical_hash, "objects.canonical_hash")?,
            revisionless_u64(&self.plaintext_length, "objects.plaintext_length")?,
        ))
    }
}

impl FileVersionRow {
    pub fn from_domain(value: FileVersion) -> Result<Self, MappingError> {
        Ok(Self {
            id: encode_id(&value.id()),
            library_id: encode_id(&value.library_id()),
            node_id: encode_id(&value.node_id()),
            object_id: encode_id(&value.object_reference().object_id()),
            object_dedup_domain_id: encode_id(&value.object_reference().dedup_domain_id()),
            parent_version_id: value.parent_version_id().map(|id| encode_id(&id)),
            committed_at: encode_timestamp(value.committed_at(), "file_versions.committed_at")?,
            revision: encode_revision(value.revision()),
        })
    }

    pub fn try_into_domain(
        self,
        library: &Library,
        node: &Node,
        object: &ObjectRow,
    ) -> Result<FileVersion, MappingError> {
        let id = decode_id!(self.id, FileVersionId, "file_versions.id")?;
        let library_id = decode_id!(self.library_id, LibraryId, "file_versions.library_id")?;
        let node_id = decode_id!(self.node_id, NodeId, "file_versions.node_id")?;
        let object_id = decode_id!(
            self.object_id,
            synveil_core::ObjectId,
            "file_versions.object_id"
        )?;
        let object_dedup_domain_id = decode_id!(
            self.object_dedup_domain_id,
            synveil_core::DedupDomainId,
            "file_versions.object_dedup_domain_id"
        )?;

        if library.id() != library_id
            || node.id() != node_id
            || node.library_id() != library.id()
            || object.id != self.object_id
            || object.dedup_domain_id != self.object_dedup_domain_id
            || object.try_into_reference()?.object_id() != object_id
            || object.try_into_reference()?.dedup_domain_id() != object_dedup_domain_id
        {
            return Err(MappingError::RelationMismatch {
                relation: "file_versions",
            });
        }

        FileVersion::rehydrate(
            id,
            library,
            node,
            object.try_into_reference()?,
            self.parent_version_id
                .map(|id| decode_id!(id, FileVersionId, "file_versions.parent_version_id"))
                .transpose()?,
            decode_timestamp(self.committed_at, "file_versions.committed_at"),
            revision_from_field(&self.revision, "file_versions.revision")?,
        )
        .map_err(MappingError::from)
    }
}

fn revision_from_field(value: &str, field: &'static str) -> Result<Revision, MappingError> {
    Revision::from_str(value).map_err(|_| MappingError::InvalidDecimal { field })
}

fn revisionless_u64(value: &str, field: &'static str) -> Result<u64, MappingError> {
    Revision::from_str(value)
        .map(Revision::get)
        .map_err(|_| MappingError::InvalidDecimal { field })
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_core::{DedupDomainId, ObjectId, Revision};

    fn timestamp() -> Timestamp {
        Timestamp::parse("2026-08-22T00:00:00.123456Z").expect("fixed timestamp is valid")
    }

    fn name(value: &str) -> LogicalName {
        LogicalName::new(value).expect("test name is valid")
    }

    fn login(value: &str) -> LoginIdentifier {
        LoginIdentifier::new(value, format!("key:{value}")).expect("test login is valid")
    }

    fn graph() -> (User, Library, Node, Node, ObjectReference, FileVersion) {
        let at = timestamp();
        let user = User::rehydrate(
            UserId::new(),
            login("alice"),
            UserStatus::Active,
            at,
            at,
            Revision::new(7),
        );
        let library_id = LibraryId::new();
        let root = Node::new_root(NodeId::new(), library_id, name("root"), at);
        let library = Library::rehydrate(
            library_id,
            user.id(),
            name("Library"),
            LibraryStatus::ReadOnly,
            &root,
            DedupDomainId::new(),
            at,
            at,
            Revision::new(3),
        )
        .expect("library is valid");
        let mut file = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::File,
            name("A/B.txt"),
            at,
        )
        .expect("file is valid");
        file.transition_state(NodeState::Trashed, at)
            .expect("file can be trashed");
        let object = ObjectReference::new(
            ObjectId::new(),
            library.dedup_domain_id(),
            Sha256Digest::from_bytes([0xabu8; 32]),
            u64::MAX,
        );
        let version = FileVersion::rehydrate(
            FileVersionId::new(),
            &library,
            &file,
            object,
            None,
            at,
            Revision::new(2),
        )
        .expect("version is valid");
        (user, library, root, file, object, version)
    }

    #[test]
    fn domain_rows_round_trip_typed_ids_timestamps_revisions_names_states_and_hashes() {
        let (user, library, root, file, object, version) = graph();
        let at = timestamp();
        let device = Device::rehydrate(
            DeviceId::new(),
            user.id(),
            name("Workstation"),
            DeviceStatus::Paused,
            at,
            at,
            Revision::new(4),
        );

        assert_eq!(
            UserRow::from_domain(&user).unwrap().try_into_domain(),
            Ok(user)
        );
        assert_eq!(
            DeviceRow::from_domain(&device).unwrap().try_into_domain(),
            Ok(device)
        );
        let library_row = LibraryRow::from_domain(&library).unwrap();
        assert_eq!(library_row.try_into_domain(&root), Ok(library.clone()));
        assert_eq!(
            NodeRow::from_domain(&file).unwrap().try_into_domain(),
            Ok(file.clone())
        );

        let object_row = ObjectRow::from_reference(object, at).unwrap();
        assert_eq!(object_row.try_into_reference(), Ok(object));
        let version_row = FileVersionRow::from_domain(version).unwrap();
        assert_eq!(
            version_row.try_into_domain(&library, &file, &object_row),
            Ok(version)
        );
        assert_eq!(
            revision_to_decimal(Revision::new(u64::MAX)),
            u64::MAX.to_string()
        );
        assert_eq!(
            sequence_from_decimal(&u64::MAX.to_string()).unwrap(),
            Sequence::new(u64::MAX)
        );
        assert!(sequence_from_decimal("-1").is_err());
        assert!(sequence_from_decimal("18446744073709551616").is_err());
        assert!(revision_from_decimal("-1").is_err());
    }

    #[test]
    fn invalid_persisted_values_are_rejected_without_narrowing() {
        let (user, _, _, _, object, _) = graph();
        let mut user_row = UserRow::from_domain(&user).unwrap();
        user_row.revision = "18446744073709551616".to_owned();
        assert!(matches!(
            user_row.try_into_domain(),
            Err(MappingError::InvalidDecimal {
                field: "users.revision"
            })
        ));

        let mut object_row = ObjectRow::from_reference(object, timestamp()).unwrap();
        object_row.plaintext_length = "-1".to_owned();
        assert!(matches!(
            object_row.try_into_reference(),
            Err(MappingError::InvalidDecimal {
                field: "objects.plaintext_length"
            })
        ));
        object_row.plaintext_length = "18446744073709551616".to_owned();
        assert!(object_row.try_into_reference().is_err());
        object_row.canonical_hash = vec![0; 31];
        assert!(matches!(
            object_row.try_into_reference(),
            Err(MappingError::InvalidDigest {
                field: "objects.canonical_hash"
            })
        ));

        let mut node_row = NodeRow::from_domain(&graph().3).unwrap();
        node_row.kind = "UNKNOWN".to_owned();
        assert!(matches!(
            node_row.try_into_domain(),
            Err(MappingError::InvalidEnum {
                field: "nodes.kind"
            })
        ));

        let mut user_row = UserRow::from_domain(&user).unwrap();
        user_row.id = Uuid::nil();
        assert!(matches!(
            user_row.try_into_domain(),
            Err(MappingError::InvalidId {
                field: "users.id",
                ..
            })
        ));

        let timestamp_with_nanos = Timestamp::parse("2026-08-22T00:00:00.123456789Z").unwrap();
        let user_with_nanos = User::new(
            UserId::new(),
            login("nanos"),
            UserStatus::Active,
            timestamp_with_nanos,
        );
        assert!(matches!(
            UserRow::from_domain(&user_with_nanos),
            Err(MappingError::TimestampPrecisionLoss {
                field: "users.created_at"
            })
        ));
    }

    #[test]
    fn domain_relationship_mismatches_are_rejected() {
        let (_, library, _, file, object, version) = graph();
        let mut object_row = ObjectRow::from_reference(object, timestamp()).unwrap();
        object_row.dedup_domain_id = DedupDomainId::new().into_uuid();
        let version_row = FileVersionRow::from_domain(version).unwrap();
        assert!(matches!(
            version_row.try_into_domain(&library, &file, &object_row),
            Err(MappingError::RelationMismatch {
                relation: "file_versions"
            }) | Err(MappingError::Domain(DomainError::ObjectDedupDomainMismatch))
        ));
    }
}
