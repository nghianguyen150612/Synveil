//! Provisional, isolated qualification model. Inputs must come from trusted
//! authentication and authority stores; this module is not production policy.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Principal {
    Browser { id: String },
    Device { id: String, device: String },
    Unauthenticated,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub host: String,
    pub owner: String,
    pub device: Option<String>,
    pub resource_type: String,
    pub resource_id: String,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub principal: Principal,
    pub binding: Binding,
    pub role: String,
    pub revision: u64,
    pub revoked: bool,
    pub expires_at: Option<u64>,
    pub deny: bool,
}

#[derive(Clone, Debug)]
pub struct Context {
    pub principal: Principal,
    pub authentication_method: String,
    pub audience: String,
    pub binding: Binding,
    pub revision: u64,
    pub principal_revoked: bool,
    pub device_revoked: bool,
    pub approval_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied,
    RequiresApproval,
    Unsupported,
    StaleOrRevoked,
}

/// Qualification only: `assignments` and supported vocabulary must be supplied
/// by a trusted policy snapshot, never by the requester. No approval is implied.
pub fn evaluate(
    context: &Context,
    policy_revision: u64,
    now: u64,
    assignments: &[Assignment],
    supported: &[(String, String, String)],
) -> Decision {
    if policy_revision != 1 || context.revision != policy_revision {
        return Decision::StaleOrRevoked;
    }
    if context.principal_revoked || context.device_revoked {
        return Decision::StaleOrRevoked;
    }
    let (id, method) = match &context.principal {
        Principal::Browser { id } if context.binding.device.is_none() => (id, "browser"),
        Principal::Device { id, device } if context.binding.device.as_ref() == Some(device) => {
            (id, "device")
        }
        Principal::Unauthenticated => return Decision::Denied,
        _ => return Decision::Unsupported,
    };
    let b = &context.binding;
    if [
        id.as_str(),
        &context.audience,
        &b.host,
        &b.owner,
        &b.resource_type,
        &b.resource_id,
        &b.action,
    ]
    .iter()
    .any(|s| s.is_empty())
        || context.authentication_method != method
        || context.audience != b.host
    {
        return Decision::Denied;
    }
    let mut allow = false;
    for a in assignments
        .iter()
        .filter(|a| a.principal == context.principal && a.binding == *b)
    {
        if a.revision != policy_revision
            || a.revoked
            || a.expires_at.is_some_and(|expiry| now >= expiry)
        {
            return Decision::StaleOrRevoked;
        }
        if !supported.iter().any(|(role, resource, action)| {
            role == &a.role && resource == &b.resource_type && action == &b.action
        }) {
            return Decision::Unsupported;
        }
        if a.deny {
            return Decision::Denied;
        }
        allow = true;
    }
    if !allow {
        return Decision::Denied;
    }
    if context.approval_required {
        Decision::RequiresApproval
    } else {
        Decision::Allowed
    }
}

/// P004-D05 is pending: this intersection is a reference rule only.
pub fn qualify_legacy(legacy_allow: bool, proposed: Decision) -> Decision {
    if !legacy_allow {
        Decision::Denied
    } else {
        proposed
    }
}
