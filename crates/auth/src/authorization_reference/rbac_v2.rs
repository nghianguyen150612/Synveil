//! P026 qualification contracts, never wired to production authentication.
//! Every Policy/Record/Actor input is an independently trusted fixture store.
//! Requesters supply only Context and grant ID; neither is authority.
use super::{Assignment, Context, Decision, Principal, evaluate, qualify_legacy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyRevision(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRevision(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    FixtureViewer,
    FixtureEditor,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    Library,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
    Delete,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub owner: OwnerId,
    pub host: InstallationId,
    pub resource_type: ResourceType,
    pub resource: ResourceId,
    pub device: Option<String>,
    pub action: Action,
}
#[derive(Clone, Debug)]
pub struct Actor {
    pub principal: Principal,
    /// Stable identity resolved by authentication, never a display name/session ID.
    pub human: String,
    pub revoked: bool,
}
#[derive(Clone, Debug)]
pub struct Record {
    pub id: String,
    pub recipient: Principal,
    pub grantor: Principal,
    pub scope: Scope,
    pub role: Role,
    pub role_revision: RoleRevision,
    pub revision: PolicyRevision,
    pub issued: u64,
    pub expires: u64,
    pub revoked: bool,
    pub deny: bool,
    pub parent: Option<String>,
    /// Attenuated maximum: exact one action, no wildcard or implied role union.
    pub maximum: Action,
}
#[derive(Clone, Debug)]
pub struct Approval {
    pub id: String,
    pub operation: String,
    pub requester: Principal,
    pub approver: Principal,
    pub scope: Scope,
    pub revision: PolicyRevision,
    pub issued: u64,
    pub expires: u64,
    pub withdrawn: bool,
    pub consumed: bool,
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub version: u32,
    pub revision: PolicyRevision,
    pub role_revision: RoleRevision,
    /// Independently resolved resource ownership and authenticated owner actor.
    pub ownership: Scope,
    pub owner_actor: Principal,
    pub actors: Vec<Actor>,
    pub records: Vec<Record>,
    /// Exact authorized approver identities and scope; never request flags.
    pub approvers: Vec<(Principal, Scope)>,
    pub approval_actions: Vec<Action>,
    pub approvals: Vec<Approval>,
    pub recursive: bool,
    pub max_depth: usize,
}
fn actor<'a>(p: &'a Policy, principal: &Principal) -> Option<&'a Actor> {
    let mut found = p.actors.iter().filter(|a| &a.principal == principal);
    let a = found.next()?;
    if found.next().is_some()
        || a.revoked
        || a.human.is_empty()
        || !matches!(principal,
        Principal::Browser { id } | Principal::Device { id, .. } if !id.is_empty())
    {
        return None;
    }
    Some(a)
}
fn role_allows(r: &Record) -> bool {
    r.scope.resource_type == ResourceType::Library
        && match r.role {
            Role::FixtureViewer => r.scope.action == Action::Read,
            Role::FixtureEditor => matches!(
                r.scope.action,
                Action::Read | Action::Write | Action::Delete
            ),
            Role::Unknown => false,
        }
}
fn binding_matches(s: &Scope, c: &Context) -> bool {
    s.host.0 == c.binding.host
        && s.owner.0 == c.binding.owner
        && s.resource.0 == c.binding.resource_id
        && c.binding.resource_type == "library"
        && s.resource_type == ResourceType::Library
        && s.device == c.binding.device
        && match s.action {
            Action::Read => c.binding.action == "read",
            Action::Write => c.binding.action == "write",
            Action::Delete => c.binding.action == "delete",
            Action::Unknown => false,
        }
}
fn chain(p: &Policy, r: &Record, now: u64, seen: &mut Vec<String>) -> Result<(), Decision> {
    if seen.contains(&r.id) || seen.len() > p.max_depth || r.id.is_empty() {
        return Err(Decision::Denied);
    }
    seen.push(r.id.clone());
    if r.revoked
        || r.revision != p.revision
        || now < r.issued
        || now >= r.expires
        || r.issued >= r.expires
    {
        return Err(Decision::StaleOrRevoked);
    }
    if r.role_revision != p.role_revision {
        return Err(Decision::StaleOrRevoked);
    }
    if !role_allows(r) {
        return Err(Decision::Unsupported);
    }
    if r.deny
        || r.maximum != r.scope.action
        || actor(p, &r.grantor).is_none()
        || actor(p, &r.recipient).is_none()
    {
        return Err(Decision::Denied);
    }
    if p.records
        .iter()
        .any(|d| d.recipient == r.recipient && d.scope == r.scope && d.deny)
    {
        return Err(Decision::Denied);
    }
    match &r.parent {
        None if r.grantor == p.owner_actor => Ok(()),
        Some(id) => {
            let parent = p
                .records
                .iter()
                .find(|x| &x.id == id)
                .ok_or(Decision::Denied)?;
            if (!p.recursive && parent.parent.is_some())
                || parent.recipient != r.grantor
                || parent.scope != r.scope
                || parent.maximum != r.maximum
                || r.issued < parent.issued
                || r.expires > parent.expires
            {
                return Err(Decision::Denied);
            }
            chain(p, parent, now, seen)
        }
        _ => Err(Decision::Denied),
    }
}
/// A decision is not reusable execution authority. The durable harness must
/// re-evaluate and consume approval in the same write transaction as its effect.
pub fn qualify(
    c: &Context,
    p: &Policy,
    grant_id: &str,
    operation: &str,
    now: u64,
    legacy: bool,
) -> Decision {
    qualify_legacy(legacy, decide(c, p, grant_id, operation, now))
}
fn decide(c: &Context, p: &Policy, grant_id: &str, operation: &str, now: u64) -> Decision {
    if p.version != 2 {
        return Decision::Unsupported;
    }
    if c.revision != p.revision.0 {
        return Decision::StaleOrRevoked;
    }
    if actor(p, &c.principal).is_none()
        || actor(p, &p.owner_actor).is_none()
        || !matches!(p.owner_actor, Principal::Browser { .. })
    {
        return Decision::Denied;
    }
    if !binding_matches(&p.ownership, c) {
        return Decision::Denied;
    }
    let mut ids = std::collections::HashSet::new();
    if p.records.iter().any(|r| !ids.insert(&r.id)) {
        return Decision::Denied;
    }
    let Some(r) = p.records.iter().find(|r| r.id == grant_id) else {
        return Decision::Denied;
    };
    if r.recipient != c.principal || !binding_matches(&r.scope, c) {
        return Decision::Denied;
    }
    if let Err(d) = chain(p, r, now, &mut Vec::new()) {
        return d;
    }
    // Reuse P025 authentication class/audience/device checks, after v2 trust checks.
    let mut normalized = c.clone();
    normalized.revision = 1;
    normalized.approval_required = false;
    let a = Assignment {
        principal: c.principal.clone(),
        binding: c.binding.clone(),
        role: "fixture-v2".into(),
        revision: 1,
        revoked: false,
        expires_at: None,
        deny: false,
    };
    let d = evaluate(
        &normalized,
        1,
        now,
        &[a],
        &[(
            "fixture-v2".into(),
            "library".into(),
            c.binding.action.clone(),
        )],
    );
    if d != Decision::Allowed {
        return d;
    }
    if !p.approval_actions.contains(&r.scope.action) {
        return Decision::Allowed;
    }
    if operation.is_empty() {
        return Decision::RequiresApproval;
    }
    let mut evidence_ids = std::collections::HashSet::new();
    if p.approvals.iter().any(|a| !evidence_ids.insert(&a.id)) {
        return Decision::RequiresApproval;
    }
    let requester = actor(p, &c.principal).expect("validated actor");
    if p.approvals.iter().any(|a| {
        let approver = actor(p, &a.approver);
        !a.id.is_empty()
            && !a.withdrawn
            && !a.consumed
            && a.operation == operation
            && a.requester == c.principal
            && a.approver != c.principal
            && matches!(a.approver, Principal::Browser { .. })
            && approver.is_some_and(|x| x.human != requester.human)
            && p.approvers.contains(&(a.approver.clone(), r.scope.clone()))
            && a.scope == r.scope
            && a.revision == p.revision
            && a.issued <= now
            && now < a.expires
    }) {
        Decision::Allowed
    } else {
        Decision::RequiresApproval
    }
}
