//! Public, profile-scoped Host trust contract. Contains no key or bearer data.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x509_parser::{parse_x509_certificate, pem::parse_x509_pem};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostTrust {
    Managed(ManagedHostTrust),
    Public(PublicHostTrust),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicHostTrust {
    pub schema_version: u32,
    pub kind: String,
    pub server_installation_id: String,
    pub network_integration_id: String,
    pub canonical_origin: String,
    pub config_generation: u64,
}
impl HostTrust {
    pub fn origin(&self) -> &str {
        match self {
            Self::Managed(t) => &t.canonical_origin,
            Self::Public(t) => &t.canonical_origin,
        }
    }
    pub fn server_id(&self) -> &str {
        match self {
            Self::Managed(t) => &t.server_installation_id,
            Self::Public(t) => &t.server_installation_id,
        }
    }
    pub fn network_id(&self) -> &str {
        match self {
            Self::Managed(t) => &t.network_integration_id,
            Self::Public(t) => &t.network_integration_id,
        }
    }
    pub fn generation(&self) -> u64 {
        match self {
            Self::Managed(t) => t.config_generation,
            Self::Public(t) => t.config_generation,
        }
    }
    pub fn ca_sha256(&self) -> Option<&str> {
        match self {
            Self::Managed(t) => Some(&t.ca_certificate_sha256),
            Self::Public(_) => None,
        }
    }
    pub fn validate(&self, origin: &str, now: i64) -> Result<Option<Vec<u8>>, &'static str> {
        match self {
            Self::Managed(t) => t.certificate_der(origin, now).map(Some),
            Self::Public(t) => {
                let parsed = url::Url::parse(origin).map_err(|_| "invalid Host origin")?;
                if t.schema_version != 1
                    || t.kind != "public_web_pki"
                    || t.config_generation == 0
                    || t.canonical_origin != origin
                    || parsed.as_str() != origin
                    || parsed.scheme() != "https"
                    || parsed.path() != "/"
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err("invalid public HTTPS Host trust");
                }
                for id in [&t.server_installation_id, &t.network_integration_id] {
                    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "invalid Host identity")?;
                    if parsed.get_version_num() != 7 || parsed.to_string() != *id {
                        return Err("invalid Host identity");
                    }
                }
                Ok(None)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedHostTrust {
    pub schema_version: u32,
    pub server_installation_id: String,
    pub network_integration_id: String,
    pub canonical_origin: String,
    pub ca_certificate_pem: String,
    pub ca_certificate_sha256: String,
    pub ca_not_before: i64,
    pub ca_not_after: i64,
    pub config_generation: u64,
}
impl ManagedHostTrust {
    pub fn from_ca(
        server_installation_id: String,
        network_integration_id: String,
        canonical_origin: String,
        ca_certificate_pem: String,
        config_generation: u64,
    ) -> Result<Self, &'static str> {
        let (_, pem) =
            parse_x509_pem(ca_certificate_pem.as_bytes()).map_err(|_| "invalid Host CA")?;
        let (_, certificate) =
            parse_x509_certificate(&pem.contents).map_err(|_| "invalid Host CA")?;
        let result = Self {
            schema_version: 1,
            server_installation_id,
            network_integration_id,
            canonical_origin,
            ca_certificate_sha256: format!("{:x}", Sha256::digest(&pem.contents)),
            ca_not_before: certificate.validity().not_before.timestamp(),
            ca_not_after: certificate.validity().not_after.timestamp(),
            ca_certificate_pem,
            config_generation,
        };
        Ok(result)
    }
    pub fn certificate_der(&self, origin: &str, now: i64) -> Result<Vec<u8>, &'static str> {
        if self.schema_version != 1
            || self.config_generation == 0
            || self.ca_certificate_pem.len() > 16 * 1024
            || self.ca_certificate_pem.contains("PRIVATE KEY")
        {
            return Err("invalid managed Host trust descriptor");
        }
        for value in [&self.server_installation_id, &self.network_integration_id] {
            let id = uuid::Uuid::parse_str(value).map_err(|_| "invalid Host identity")?;
            if id.get_version_num() != 7 || id.to_string() != *value {
                return Err("invalid Host identity");
            }
        }
        let url = url::Url::parse(&self.canonical_origin).map_err(|_| "invalid Host origin")?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || self.canonical_origin != url.as_str()
            || origin != self.canonical_origin
        {
            return Err("Host trust origin mismatch");
        }
        let (remaining, pem) = parse_x509_pem(self.ca_certificate_pem.as_bytes())
            .map_err(|_| "invalid Host CA certificate")?;
        if remaining.iter().any(|b| !b.is_ascii_whitespace()) || pem.label != "CERTIFICATE" {
            return Err("exactly one CA certificate required");
        }
        let (_, certificate) =
            parse_x509_certificate(&pem.contents).map_err(|_| "invalid Host CA certificate")?;
        if certificate.subject() != certificate.issuer()
            || !certificate
                .basic_constraints()
                .map_err(|_| "invalid CA constraints")?
                .is_some_and(|b| b.value.ca)
            || !certificate
                .key_usage()
                .map_err(|_| "invalid CA usage")?
                .is_some_and(|b| b.value.key_cert_sign())
            || certificate.verify_signature(None).is_err()
            || self.ca_not_before != certificate.validity().not_before.timestamp()
            || self.ca_not_after != certificate.validity().not_after.timestamp()
            || now < self.ca_not_before
            || now >= self.ca_not_after
            || format!("{:x}", Sha256::digest(&pem.contents)) != self.ca_certificate_sha256
        {
            return Err("Host CA identity or validity mismatch");
        }
        Ok(pem.contents)
    }
}
