use synveil_auth::authorization_reference::{rbac_v2::*, *};
pub fn browser(id: &str) -> Principal {
    Principal::Browser { id: id.into() }
}
pub fn fixture() -> (Context, Policy) {
    let scope = Scope {
        owner: OwnerId("owner-record".into()),
        host: InstallationId("host".into()),
        resource_type: ResourceType::Library,
        resource: ResourceId("lib".into()),
        device: None,
        action: Action::Read,
    };
    let c = Context {
        principal: browser("alice"),
        authentication_method: "browser".into(),
        audience: "host".into(),
        binding: Binding {
            owner: "owner-record".into(),
            host: "host".into(),
            resource_type: "library".into(),
            resource_id: "lib".into(),
            device: None,
            action: "read".into(),
        },
        revision: 7,
        principal_revoked: false,
        device_revoked: false,
        approval_required: false,
    };
    let r = Record {
        id: "root".into(),
        recipient: browser("alice"),
        grantor: browser("owner-actor"),
        scope: scope.clone(),
        role: Role::FixtureViewer,
        role_revision: RoleRevision(1),
        revision: PolicyRevision(7),
        issued: 1,
        expires: 100,
        revoked: false,
        deny: false,
        parent: None,
        maximum: Action::Read,
    };
    let p = Policy {
        version: 2,
        revision: PolicyRevision(7),
        role_revision: RoleRevision(1),
        ownership: scope,
        owner_actor: browser("owner-actor"),
        actors: ["owner-actor", "alice", "bob", "carol"]
            .iter()
            .map(|id| Actor {
                principal: browser(id),
                human: id.to_string(),
                revoked: false,
            })
            .collect(),
        records: vec![r],
        approvers: vec![],
        approval_actions: vec![],
        approvals: vec![],
        recursive: false,
        max_depth: 2,
    };
    (c, p)
}
