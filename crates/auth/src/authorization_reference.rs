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
        Principal::Device { id, device }
            if !device.is_empty() && context.binding.device.as_ref() == Some(device) =>
        {
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

/// Trusted ownership record, resolved independently of the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership {
    pub host: String,
    pub resource_type: String,
    pub resource_id: String,
    pub owner: String,
}

/// Provenance is a qualification input from a trusted authority snapshot. It is
/// not a claim accepted from an HTTP request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorityProvenance {
    Owner { owner: String },
    Delegated { grantor: String, owner: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub assignment: Assignment,
    pub provenance: AuthorityProvenance,
}

/// Evidence is supplied by an independently trusted approval store. This type
/// does not implement an approval protocol, signatures, or single-use execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalEvidence {
    pub approver: Principal,
    pub requester: Principal,
    pub binding: Binding,
    pub revision: u64,
    pub expires_at: u64,
    pub revoked: bool,
    pub approver_authorized: bool,
    /// Trusted stable human identities, not session IDs.
    pub approver_actor: String,
    pub requester_actor: String,
}

/// A single trusted and consistently read snapshot. Its provenance and atomic
/// acquisition are responsibilities of a future production implementation.
pub struct AuthoritySnapshot<'a> {
    pub policy_version: u32,
    pub revision: u64,
    pub ownership: &'a Ownership,
    pub grants: &'a [Grant],
    pub vocabulary: &'a [(String, String, String)],
    pub approval: Option<&'a ApprovalEvidence>,
}

/// Reference qualification only. No caller-supplied claims may populate the
/// snapshot, approval requirement, principal, revocation or ownership fields.
pub fn evaluate_snapshot(
    context: &Context,
    now: u64,
    snapshot: &AuthoritySnapshot<'_>,
) -> Decision {
    if snapshot.policy_version != 1 {
        return Decision::Unsupported;
    }
    if context.revision != snapshot.revision || context.principal_revoked || context.device_revoked
    {
        return Decision::StaleOrRevoked;
    }
    let b = &context.binding;
    let ownership = snapshot.ownership;
    if ownership.host != b.host
        || ownership.owner != b.owner
        || ownership.resource_type != b.resource_type
        || ownership.resource_id != b.resource_id
        || ownership.owner.is_empty()
    {
        return Decision::Denied;
    }
    let matching: Vec<&Grant> = snapshot
        .grants
        .iter()
        .filter(|g| g.assignment.principal == context.principal && g.assignment.binding == *b)
        .collect();
    if matching.iter().any(|g| match &g.provenance {
        AuthorityProvenance::Owner { owner } => owner != &ownership.owner,
        AuthorityProvenance::Delegated { grantor, owner } => {
            owner != &ownership.owner || grantor != owner
        }
    }) {
        return Decision::Denied;
    }
    // A valid explicit deny wins regardless of ordering; stale or invalid
    // records still fail closed even in the presence of an allow.
    if matching.iter().any(|g| {
        g.assignment.revision != snapshot.revision
            || g.assignment.revoked
            || g.assignment.expires_at.is_some_and(|expiry| now >= expiry)
    }) {
        return Decision::StaleOrRevoked;
    }
    if matching.iter().any(|g| {
        !snapshot.vocabulary.iter().any(|(role, resource, action)| {
            role == &g.assignment.role && resource == &b.resource_type && action == &b.action
        })
    }) {
        return Decision::Unsupported;
    }
    if matching.iter().any(|g| g.assignment.deny) {
        return Decision::Denied;
    }
    let assignments: Vec<Assignment> = matching.into_iter().map(|g| g.assignment.clone()).collect();
    // The original helper's version marker is 1, whereas snapshot revision is
    // an independently changing generation. Normalize only after validation.
    let mut normalized = context.clone();
    normalized.revision = 1;
    let normalized_assignments: Vec<Assignment> = assignments
        .into_iter()
        .map(|mut a| {
            a.revision = 1;
            a
        })
        .collect();
    let decision = evaluate(
        &normalized,
        1,
        now,
        &normalized_assignments,
        snapshot.vocabulary,
    );
    if decision != Decision::RequiresApproval {
        return decision;
    }
    match snapshot.approval {
        Some(a)
            if !a.revoked
                && a.approver_authorized
                && !a.approver_actor.is_empty()
                && !a.requester_actor.is_empty()
                && a.approver_actor != a.requester_actor
                && a.approver != context.principal
                && matches!(a.approver, Principal::Browser { ref id } if !id.is_empty())
                && a.requester == context.principal
                && a.binding == *b
                && a.revision == snapshot.revision
                && now < a.expires_at =>
        {
            Decision::Allowed
        }
        _ => Decision::RequiresApproval,
    }
}

/// Application policy never provides OS privilege. An OS-specific authorization
/// must be checked separately by the privileged effect's implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsPrivilege {
    NotGranted,
}

pub fn application_decision_os_privilege(_: Decision) -> OsPrivilege {
    OsPrivilege::NotGranted
}

/// P026 versioned qualification extends this reference boundary only.
pub mod rbac_v2;
