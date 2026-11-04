use synveil_auth::authorization_reference::{rbac_v2::*, *};
#[path = "support/p026.rs"]
mod fixtures;
use fixtures::{browser, fixture};
#[test]
fn exact_scope_security_matrix() {
    let (c, p) = fixture();
    assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Allowed);
    for case in 0..29 {
        let (mut c, mut p) = fixture();
        let expected = match case {
            0 => {
                c.principal = Principal::Unauthenticated;
                Decision::Denied
            }
            1 => {
                c.principal = Principal::Unknown;
                Decision::Denied
            }
            2 => {
                c.authentication_method = "device".into();
                Decision::Denied
            }
            3 => {
                c.binding.owner = "other".into();
                Decision::Denied
            }
            4 => {
                p.records[0].role = Role::Unknown;
                Decision::Unsupported
            }
            5 => {
                p.records[0].grantor = browser("owner-record");
                Decision::Denied
            }
            6 => {
                c.binding.host = "other".into();
                c.audience = "other".into();
                Decision::Denied
            }
            7 => {
                c.binding.resource_id = "other".into();
                Decision::Denied
            }
            8 => {
                c.binding.action = "unknown".into();
                Decision::Denied
            }
            9 => {
                p.version = 3;
                Decision::Unsupported
            }
            10 => {
                p.records[0].revision = PolicyRevision(6);
                Decision::StaleOrRevoked
            }
            11 => {
                p.records[0].expires = 10;
                Decision::StaleOrRevoked
            }
            12 => {
                p.records[0].revoked = true;
                Decision::StaleOrRevoked
            }
            13 => {
                let mut deny = p.records[0].clone();
                deny.id = "deny".into();
                deny.deny = true;
                p.records.push(deny);
                Decision::Denied
            }
            14 => {
                p.records[0].grantor = browser("bob");
                Decision::Denied
            }
            15 => {
                p.records[0].maximum = Action::Write;
                Decision::Denied
            }
            16 => {
                c.revision = 6;
                Decision::StaleOrRevoked
            }
            17 => {
                c.binding.device = Some("foreign".into());
                Decision::Denied
            }
            18 => {
                c.binding.action = "os-root".into();
                Decision::Denied
            }
            19 => {
                p.records[0].role_revision = RoleRevision(2);
                Decision::StaleOrRevoked
            }
            20 => {
                c.principal_revoked = true;
                Decision::StaleOrRevoked
            }
            21 => {
                c.device_revoked = true;
                Decision::StaleOrRevoked
            }
            22 => {
                p.actors[0].revoked = true;
                Decision::Denied
            }
            23 => {
                p.records[0].issued = 11;
                Decision::StaleOrRevoked
            }
            24 => {
                p.records.push(p.records[0].clone());
                Decision::Denied
            }
            25 => {
                p.records[0].scope.resource_type = ResourceType::Unknown;
                Decision::Denied
            }
            26 => {
                p.records[0].scope.action = Action::Unknown;
                Decision::Denied
            }
            27 => {
                c.audience = "foreign".into();
                Decision::Denied
            }
            _ => {
                p.records.clear();
                Decision::Denied
            }
        };
        assert_eq!(
            qualify(&c, &p, "root", "op", 10, true),
            expected,
            "case {case}"
        );
    }
    assert_eq!(qualify(&c, &p, "root", "op", 10, false), Decision::Denied);
    assert_eq!(
        application_decision_os_privilege(Decision::Allowed),
        OsPrivilege::NotGranted
    );
}
fn delegated() -> (Context, Policy) {
    let (mut c, mut p) = fixture();
    let mut child = p.records[0].clone();
    child.id = "child".into();
    child.grantor = browser("alice");
    child.recipient = browser("bob");
    child.parent = Some("root".into());
    child.expires = 90;
    p.records.push(child);
    c.principal = browser("bob");
    (c, p)
}
#[test]
fn delegation_attenuation_and_provenance_matrix() {
    let (c, p) = delegated();
    assert_eq!(qualify(&c, &p, "child", "op", 10, true), Decision::Allowed);
    for case in 0..13 {
        let (c, mut p) = delegated();
        let expected = match case {
            0 => {
                p.records[0].revoked = true;
                Decision::StaleOrRevoked
            }
            1 => {
                p.records[0].expires = 10;
                Decision::Denied
            } // child outlives parent
            2 => {
                p.records[1].scope.owner = OwnerId("foreign".into());
                Decision::Denied
            }
            3 => {
                p.records[1].scope.host = InstallationId("foreign".into());
                Decision::Denied
            }
            4 => {
                p.records[1].maximum = Action::Delete;
                Decision::Denied
            }
            5 => {
                p.records[1].grantor = browser("carol");
                Decision::Denied
            }
            6 => {
                p.records[1].parent = Some("child".into());
                Decision::Denied
            }
            7 => {
                p.max_depth = 0;
                Decision::Denied
            }
            8 => {
                p.records[0].parent = Some("child".into());
                p.records[0].grantor = browser("bob");
                p.records[0].expires = 90;
                p.recursive = true;
                Decision::Denied
            }
            9 => {
                p.records[0].revision = PolicyRevision(6);
                Decision::StaleOrRevoked
            }
            10 => {
                p.records[1].expires = 101;
                Decision::Denied
            }
            11 => {
                p.actors[1].revoked = true;
                Decision::Denied
            }
            _ => {
                let malformed = Principal::Device {
                    id: "malformed".into(),
                    device: "".into(),
                };
                p.actors.push(Actor {
                    principal: malformed.clone(),
                    human: "malformed".into(),
                    revoked: false,
                });
                p.records[0].recipient = malformed.clone();
                p.records[1].grantor = malformed;
                Decision::Denied
            }
        };
        assert_eq!(
            qualify(&c, &p, "child", "op", 10, true),
            expected,
            "delegation {case}"
        );
    }
    let (mut c, mut p) = delegated();
    let mut third = p.records[1].clone();
    third.id = "third".into();
    third.parent = Some("child".into());
    third.grantor = browser("bob");
    third.recipient = browser("carol");
    p.records.push(third);
    c.principal = browser("carol");
    assert_eq!(qualify(&c, &p, "third", "op", 10, true), Decision::Denied);
    p.recursive = true;
    assert_eq!(qualify(&c, &p, "third", "op", 10, true), Decision::Allowed);
    p.records[0].revoked = true;
    assert_eq!(
        qualify(&c, &p, "third", "op", 10, true),
        Decision::StaleOrRevoked
    );
}
#[test]
fn approval_is_policy_derived_and_independent() {
    let (c, mut p) = fixture();
    p.approval_actions = vec![Action::Read];
    assert!(!c.approval_required);
    assert_eq!(
        qualify(&c, &p, "root", "op", 10, true),
        Decision::RequiresApproval
    );
    p.approvers.push((browser("bob"), p.ownership.clone()));
    p.approvals.push(Approval {
        id: "evidence".into(),
        operation: "op".into(),
        requester: c.principal.clone(),
        approver: browser("bob"),
        scope: p.ownership.clone(),
        revision: p.revision,
        issued: 1,
        expires: 20,
        withdrawn: false,
        consumed: false,
    });
    assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Allowed);
    for case in 0..13 {
        let mut changed = p.clone();
        match case {
            0 => changed.approvals[0].approver = c.principal.clone(),
            1 => changed.approvals[0].expires = 10,
            2 => changed.approvals[0].scope.host = InstallationId("foreign".into()),
            3 => changed.approvals[0].scope.resource = ResourceId("foreign".into()),
            4 => changed.approvals[0].consumed = true,
            5 => changed.approvals[0].withdrawn = true,
            6 => changed.approvals[0].revision = PolicyRevision(6),
            7 => changed.approvals[0].operation = "replay".into(),
            8 => changed.approvers.clear(),
            9 => changed.actors[2].human = "alice".into(),
            10 => changed.approvals[0].issued = 11,
            11 => changed.approvals[0].requester = browser("carol"),
            _ => changed.approvals.push(changed.approvals[0].clone()),
        }
        assert_eq!(
            qualify(&c, &changed, "root", "op", 10, true),
            Decision::RequiresApproval,
            "approval {case}"
        );
    }
}
#[test]
fn device_exact_binding_does_not_create_browser_authority() {
    let (mut c, mut p) = fixture();
    let device = Principal::Device {
        id: "credential".into(),
        device: "device".into(),
    };
    c.principal = device.clone();
    c.authentication_method = "device".into();
    c.binding.device = Some("device".into());
    p.ownership.device = c.binding.device.clone();
    p.records[0].scope = p.ownership.clone();
    p.records[0].recipient = device.clone();
    p.actors.push(Actor {
        principal: device,
        human: "device-actor".into(),
        revoked: false,
    });
    assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Allowed);
    c.binding.device = Some("other".into());
    assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Denied);
    c.binding.device = Some("device".into());
    c.authentication_method = "browser".into();
    assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Denied);
}

#[test]
fn fixture_editor_permissions_and_order_independent_denials() {
    for (action, name) in [
        (Action::Read, "read"),
        (Action::Write, "write"),
        (Action::Delete, "delete"),
    ] {
        let (mut c, mut p) = fixture();
        c.binding.action = name.into();
        p.ownership.action = action;
        p.records[0].scope.action = action;
        p.records[0].maximum = action;
        p.records[0].role = Role::FixtureEditor;
        assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Allowed);
        if action != Action::Read {
            p.records[0].role = Role::FixtureViewer;
            assert_eq!(
                qualify(&c, &p, "root", "op", 10, true),
                Decision::Unsupported
            );
            p.records[0].role = Role::FixtureEditor;
        }
        let mut deny = p.records[0].clone();
        deny.id = "deny".into();
        deny.deny = true;
        p.records.push(deny);
        assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Denied);
        p.records.reverse();
        assert_eq!(qualify(&c, &p, "root", "op", 10, true), Decision::Denied);
    }
}
