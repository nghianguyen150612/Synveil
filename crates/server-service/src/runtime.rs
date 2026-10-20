//! Closed runtime catalog shared by the producer and the privileged consumer.
//! A digest supplied by an unprivileged caller is never a trust anchor.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub kind: String,
    pub version: String,
    pub packaging_revision: String,
    pub platform: String,
    pub architecture: String,
    pub qualified_os: String,
    pub compatibility: String,
    pub usage: String,
    pub upstream_url: String,
    pub upstream_revision: String,
    pub provenance_sha256: String,
    pub license: RuntimeLicense,
    pub files: Vec<RuntimeFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLicense {
    pub spdx_id: String,
    pub license_file: String,
    pub license_sha256: String,
    pub notice_file: String,
    pub notice_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProvenance {
    pub schema_version: u32,
    pub component: String,
    pub version: String,
    pub source_revision: String,
    pub source_url: String,
    pub platform: String,
    pub architecture: String,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl RuntimeManifest {
    pub fn validate(&self, os: &str) -> Result<(), &'static str> {
        let valid_upstream_url = url::Url::parse(&self.upstream_url).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        });
        let exact_version = self.version.len() <= 64
            && !self.version.to_ascii_lowercase().contains("latest")
            && !["unknown", "unverified", "placeholder", "0.0.0"]
                .contains(&self.version.to_ascii_lowercase().as_str())
            && self
                .version
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && self.version.contains('.')
            && !self.version.ends_with('.')
            && !self.version.ends_with('-')
            && !self.version.ends_with('+')
            && !self.version.contains("..")
            && self
                .version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte));
        let expected_license = match self.kind.as_str() {
            "postgresql" => "PostgreSQL",
            "caddy" => "Apache-2.0",
            "server" => "MIT",
            _ => return Err("unsupported runtime kind/version"),
        };
        let supported_major = self.kind != "postgresql" || self.version.starts_with("17.");
        if self.schema_version != 2
            || self.platform != "linux"
            || self.architecture != "x86_64"
            || self.qualified_os != os
            || !["ubuntu-24.04", "fedora-42"].contains(&os)
            || self.compatibility != "synveil-desktop-server-v0.2"
            || self.usage != "managed_host_production_input"
            || !exact_version
            || !supported_major
            || self.packaging_revision.len() != 40
            || !self
                .packaging_revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.upstream_revision.len() != 40
            || !self
                .upstream_revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.upstream_url.len() > 2048
            || !valid_upstream_url
            || !digest_valid(&self.provenance_sha256)
            || self.license.spdx_id != expected_license
            || self.license.license_file != "LICENSE"
            || self.license.notice_file != "NOTICE"
            || !digest_valid(&self.license.license_sha256)
            || !digest_valid(&self.license.notice_sha256)
            || self.files.is_empty()
            || self.files.len() > 8192
        {
            return Err("invalid runtime qualification");
        }
        let required: &[&str] = match self.kind.as_str() {
            "postgresql" if self.version.starts_with("17.") => &[
                "bin/postgres",
                "bin/initdb",
                "bin/pg_ctl",
                "bin/pg_controldata",
                "bin/psql",
                "LICENSE",
                "NOTICE",
                "PROVENANCE",
            ],
            "caddy" => &["bin/caddy", "LICENSE", "NOTICE", "PROVENANCE"],
            "server" => &[
                "bin/synveil-api",
                "bin/synveil-worker",
                "bin/synveil-server-migrate",
                "LICENSE",
                "NOTICE",
                "PROVENANCE",
            ],
            _ => return Err("unsupported runtime kind/version"),
        };
        let mut paths = BTreeSet::new();
        let mut total_bytes = 0u64;
        for file in &self.files {
            total_bytes = total_bytes
                .checked_add(file.bytes)
                .ok_or("runtime byte count overflow")?;
            if total_bytes > 2 * 1024 * 1024 * 1024 {
                return Err("runtime byte count exceeds bound");
            }
            if file.path.len() > 512
                || file.path.is_empty()
                || file.bytes > 512 * 1024 * 1024
                || !digest_valid(&file.sha256)
                || !paths.insert(file.path.as_str())
                || Path::new(&file.path)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err("invalid closed runtime inventory");
            }
        }
        if required.iter().any(|name| !paths.contains(name)) {
            return Err("incomplete runtime inventory");
        }
        if required.iter().any(|name| {
            self.files
                .iter()
                .find(|file| file.path == *name)
                .is_none_or(|file| {
                    file.bytes == 0 || (name.starts_with("bin/") && !file.executable)
                })
        }) {
            return Err("required runtime program/license is empty or not executable");
        }
        if self
            .files
            .iter()
            .find(|file| file.path == "PROVENANCE")
            .map(|file| &file.sha256)
            != Some(&self.provenance_sha256)
        {
            return Err("provenance identity mismatch");
        }
        if self
            .files
            .iter()
            .find(|file| file.path == self.license.license_file)
            .map(|file| &file.sha256)
            != Some(&self.license.license_sha256)
            || self
                .files
                .iter()
                .find(|file| file.path == self.license.notice_file)
                .map(|file| &file.sha256)
                != Some(&self.license.notice_sha256)
        {
            return Err("license or notice identity mismatch");
        }
        Ok(())
    }

    pub fn validate_provenance(&self, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() > 1024 * 1024 {
            return Err("runtime provenance exceeds bound");
        }
        let provenance: RuntimeProvenance =
            serde_json::from_slice(bytes).map_err(|_| "invalid runtime provenance")?;
        if provenance.schema_version != 1
            || provenance.component != self.kind
            || provenance.version != self.version
            || provenance.source_revision != self.upstream_revision
            || provenance.source_url != self.upstream_url
            || provenance.platform != self.qualified_os
            || provenance.architecture != self.architecture
        {
            return Err("runtime provenance identity mismatch");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> RuntimeManifest {
        let license_sha = "1".repeat(64);
        let notice_sha = "2".repeat(64);
        let provenance_sha = "3".repeat(64);
        let mut files = vec![
            RuntimeFile {
                path: "bin/postgres".into(),
                sha256: "4".repeat(64),
                bytes: 1,
                executable: true,
            },
            RuntimeFile {
                path: "bin/initdb".into(),
                sha256: "5".repeat(64),
                bytes: 1,
                executable: true,
            },
            RuntimeFile {
                path: "bin/pg_ctl".into(),
                sha256: "6".repeat(64),
                bytes: 1,
                executable: true,
            },
            RuntimeFile {
                path: "bin/pg_controldata".into(),
                sha256: "7".repeat(64),
                bytes: 1,
                executable: true,
            },
            RuntimeFile {
                path: "bin/psql".into(),
                sha256: "8".repeat(64),
                bytes: 1,
                executable: true,
            },
            RuntimeFile {
                path: "LICENSE".into(),
                sha256: license_sha.clone(),
                bytes: 1,
                executable: false,
            },
            RuntimeFile {
                path: "NOTICE".into(),
                sha256: notice_sha.clone(),
                bytes: 1,
                executable: false,
            },
            RuntimeFile {
                path: "PROVENANCE".into(),
                sha256: provenance_sha.clone(),
                bytes: 1,
                executable: false,
            },
        ];
        files.sort_by(|left, right| left.path.cmp(&right.path));
        RuntimeManifest {
            schema_version: 2,
            kind: "postgresql".into(),
            version: "17.6".into(),
            packaging_revision: "a".repeat(40),
            platform: "linux".into(),
            architecture: "x86_64".into(),
            qualified_os: "ubuntu-24.04".into(),
            compatibility: "synveil-desktop-server-v0.2".into(),
            usage: "managed_host_production_input".into(),
            upstream_url: "https://www.postgresql.org/".into(),
            upstream_revision: "c".repeat(40),
            provenance_sha256: provenance_sha,
            license: RuntimeLicense {
                spdx_id: "PostgreSQL".into(),
                license_file: "LICENSE".into(),
                license_sha256: license_sha,
                notice_file: "NOTICE".into(),
                notice_sha256: notice_sha,
            },
            files,
        }
    }

    #[test]
    fn managed_runtime_requires_exact_major_version_and_license_evidence() {
        assert_eq!(manifest().validate("ubuntu-24.04"), Ok(()));

        let mut wrong_license = manifest();
        wrong_license.license.spdx_id = "MIT".into();
        assert_eq!(
            wrong_license.validate("ubuntu-24.04"),
            Err("invalid runtime qualification")
        );

        let mut wrong_license_digest = manifest();
        wrong_license_digest.license.license_sha256 = "9".repeat(64);
        assert_eq!(
            wrong_license_digest.validate("ubuntu-24.04"),
            Err("license or notice identity mismatch")
        );

        let mut wrong_major = manifest();
        wrong_major.version = "18.0".into();
        assert_eq!(
            wrong_major.validate("ubuntu-24.04"),
            Err("invalid runtime qualification")
        );

        let qualified = manifest();
        let provenance = serde_json::json!({
            "schema_version": 1,
            "component": "postgresql",
            "version": "17.6",
            "source_revision": "c".repeat(40),
            "source_url": "https://www.postgresql.org/",
            "platform": "ubuntu-24.04",
            "architecture": "x86_64"
        });
        let bytes = serde_json::to_vec(&provenance).unwrap();
        assert_eq!(qualified.validate_provenance(&bytes), Ok(()));

        let mut mismatch = provenance;
        mismatch["source_revision"] = serde_json::json!("d".repeat(40));
        let bytes = serde_json::to_vec(&mismatch).unwrap();
        assert_eq!(
            qualified.validate_provenance(&bytes),
            Err("runtime provenance identity mismatch")
        );
    }
}
