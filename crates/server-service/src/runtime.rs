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
    pub files: Vec<RuntimeFile>,
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
        if self.schema_version != 1
            || self.platform != "linux"
            || self.architecture != "x86_64"
            || self.qualified_os != os
            || !["ubuntu-24.04", "fedora-42"].contains(&os)
            || self.compatibility != "synveil-desktop-server-v0.2"
            || self.usage != "managed_host_production_input"
            || self.version.is_empty()
            || self.version.contains("latest")
            || self.packaging_revision.len() != 40
            || !self
                .packaging_revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.upstream_revision.is_empty()
            || !self.upstream_url.starts_with("https://")
            || !digest_valid(&self.provenance_sha256)
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
        Ok(())
    }
}
