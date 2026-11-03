use synveil_auth::authorization_reference::{
    Assignment, Binding, Context, Decision, Principal, evaluate, qualify_legacy,
};

#[test]
fn qualification_fails_closed_and_intersects_legacy() {
    let binding = Binding {
        host: "host-a".into(),
        owner: "owner-a".into(),
        device: None,
        resource_type: "library".into(),
        resource_id: "library-a".into(),
        action: "read".into(),
    };
    let principal = Principal::Browser {
        id: "session-a".into(),
    };
    let mut c = Context {
        principal: principal.clone(),
        authentication_method: "browser".into(),
        audience: "host-a".into(),
        binding: binding.clone(),
        revision: 1,
        principal_revoked: false,
        device_revoked: false,
        approval_required: false,
    };
    let a = Assignment {
        principal,
        binding,
        role: "fixture".into(),
        revision: 1,
        revoked: false,
        expires_at: Some(10),
        deny: false,
    };
    let vocabulary = vec![("fixture".into(), "library".into(), "read".into())];
    let check =
        |c: &Context, assignments: &[Assignment]| evaluate(c, 1, 1, assignments, &vocabulary);
    assert_eq!(check(&c, std::slice::from_ref(&a)), Decision::Allowed);
    for (legacy, proposed, result) in [
        (true, Decision::Allowed, Decision::Allowed),
        (true, Decision::Denied, Decision::Denied),
        (false, Decision::Allowed, Decision::Denied),
        (false, Decision::Denied, Decision::Denied),
    ] {
        assert_eq!(qualify_legacy(legacy, proposed), result);
    }
    for field in 0..5 {
        let mut altered = c.clone();
        match field {
            0 => altered.binding.owner = "other".into(),
            1 => altered.binding.resource_id = "other".into(),
            2 => altered.binding.host = "other".into(),
            3 => altered.binding.action = "root".into(),
            _ => altered.principal = Principal::Unknown,
        };
        assert_ne!(check(&altered, std::slice::from_ref(&a)), Decision::Allowed);
    }
    let mut deny = a.clone();
    deny.deny = true;
    assert_eq!(check(&c, &[a.clone(), deny]), Decision::Denied);
    let mut revoked = a.clone();
    revoked.revoked = true;
    assert_eq!(check(&c, &[revoked]), Decision::StaleOrRevoked);
    assert_eq!(
        evaluate(&c, 1, 10, std::slice::from_ref(&a), &vocabulary),
        Decision::StaleOrRevoked
    );
    c.approval_required = true;
    assert_eq!(
        check(&c, std::slice::from_ref(&a)),
        Decision::RequiresApproval
    );
    c.principal_revoked = true;
    assert_eq!(check(&c, &[a]), Decision::StaleOrRevoked);
}

#[test]
fn snapshot_security_matrix() {
    use synveil_auth::authorization_reference::{
        ApprovalEvidence, AuthorityProvenance, AuthoritySnapshot, Grant, OsPrivilege, Ownership,
        application_decision_os_privilege, evaluate_snapshot,
    };
    let binding = Binding {
        host: "host-a".into(),
        owner: "owner-a".into(),
        device: None,
        resource_type: "library".into(),
        resource_id: "lib-a".into(),
        action: "read".into(),
    };
    let principal = Principal::Browser { id: "alice".into() };
    let context = Context {
        principal: principal.clone(),
        authentication_method: "browser".into(),
        audience: "host-a".into(),
        binding: binding.clone(),
        revision: 7,
        principal_revoked: false,
        device_revoked: false,
        approval_required: false,
    };
    let ownership = Ownership {
        host: "host-a".into(),
        owner: "owner-a".into(),
        resource_type: "library".into(),
        resource_id: "lib-a".into(),
    };
    let assignment = Assignment {
        principal: principal.clone(),
        binding: binding.clone(),
        role: "fixture".into(),
        revision: 7,
        revoked: false,
        expires_at: Some(10),
        deny: false,
    };
    let grant = Grant {
        assignment,
        provenance: AuthorityProvenance::Delegated {
            grantor: "owner-a".into(),
            owner: "owner-a".into(),
        },
    };
    let vocabulary = vec![("fixture".into(), "library".into(), "read".into())];
    let run = |c: &Context, grants: &[Grant], approval: Option<&ApprovalEvidence>| {
        evaluate_snapshot(
            c,
            1,
            &AuthoritySnapshot {
                policy_version: 1,
                revision: 7,
                ownership: &ownership,
                grants,
                vocabulary: &vocabulary,
                approval,
            },
        )
    };
    assert_eq!(
        run(&context, std::slice::from_ref(&grant), None),
        Decision::Allowed
    );
    let mut cases = Vec::new();
    let mut c = context.clone();
    c.principal = Principal::Unknown;
    cases.push(("unknown", c));
    let mut c = context.clone();
    c.principal = Principal::Unauthenticated;
    cases.push(("unauthenticated", c));
    let mut c = context.clone();
    c.authentication_method = "device".into();
    cases.push(("principal confusion", c));
    let mut c = context.clone();
    c.binding.owner = "owner-b".into();
    cases.push(("cross owner", c));
    let mut c = context.clone();
    c.binding.resource_id = "lib-b".into();
    cases.push(("replay resource", c));
    let mut c = context.clone();
    c.binding.host = "host-b".into();
    c.audience = "host-b".into();
    cases.push(("cross host", c));
    let mut c = context.clone();
    c.binding.device = Some("device-b".into());
    cases.push(("device impersonation", c));
    let mut c = context.clone();
    c.principal_revoked = true;
    cases.push(("revoked principal", c));
    let mut c = context.clone();
    c.device_revoked = true;
    cases.push(("revoked device", c));
    let mut c = context.clone();
    c.revision = 6;
    cases.push(("stale revision", c));
    let mut c = context.clone();
    c.binding.action = "unknown".into();
    cases.push(("unknown action", c));
    let mut c = context.clone();
    c.binding.resource_type = "unknown".into();
    cases.push(("unknown resource", c));
    let mut c = context.clone();
    c.binding.action = "os-root".into();
    cases.push(("privilege escalation", c));
    for (name, c) in cases {
        assert_ne!(
            run(&c, std::slice::from_ref(&grant), None),
            Decision::Allowed,
            "{name}"
        );
    }
    assert_eq!(
        run(&context, &[], None),
        Decision::Denied,
        "missing authority"
    );
    let mut changed = grant.clone();
    changed.assignment.role = "forged-admin".into();
    assert_eq!(run(&context, &[changed], None), Decision::Unsupported);
    let mut changed = grant.clone();
    changed.assignment.revoked = true;
    assert_eq!(run(&context, &[changed], None), Decision::StaleOrRevoked);
    let mut changed = grant.clone();
    changed.assignment.expires_at = Some(1);
    assert_eq!(run(&context, &[changed], None), Decision::StaleOrRevoked);
    let mut changed = grant.clone();
    changed.provenance = AuthorityProvenance::Delegated {
        grantor: "attacker".into(),
        owner: "owner-a".into(),
    };
    assert_eq!(run(&context, &[changed], None), Decision::Denied);
    let mut deny = grant.clone();
    deny.assignment.deny = true;
    assert_eq!(
        run(&context, &[grant.clone(), deny.clone()], None),
        Decision::Denied
    );
    assert_eq!(
        run(&context, &[deny, grant.clone()], None),
        Decision::Denied
    );
    let mut approval_context = context.clone();
    approval_context.approval_required = true;
    assert_eq!(
        run(&approval_context, std::slice::from_ref(&grant), None),
        Decision::RequiresApproval
    );
    let mut approval = ApprovalEvidence {
        approver: Principal::Browser { id: "bob".into() },
        requester: principal,
        binding,
        revision: 7,
        expires_at: 10,
        revoked: false,
        approver_authorized: true,
        approver_actor: "bob".into(),
        requester_actor: "alice".into(),
    };
    assert_eq!(
        run(
            &approval_context,
            std::slice::from_ref(&grant),
            Some(&approval)
        ),
        Decision::Allowed
    );
    approval.binding.resource_id = "lib-b".into();
    assert_eq!(
        run(
            &approval_context,
            std::slice::from_ref(&grant),
            Some(&approval)
        ),
        Decision::RequiresApproval
    );
    approval.binding = approval_context.binding.clone();
    approval.approver_actor = approval.requester_actor.clone();
    assert_eq!(
        run(
            &approval_context,
            std::slice::from_ref(&grant),
            Some(&approval)
        ),
        Decision::RequiresApproval
    );
    let mut device_context = context.clone();
    device_context.principal = Principal::Device {
        id: "credential-a".into(),
        device: "device-a".into(),
    };
    device_context.authentication_method = "device".into();
    device_context.binding.device = Some("device-a".into());
    let mut device_grant = grant.clone();
    device_grant.assignment.principal = device_context.principal.clone();
    device_grant.assignment.binding = device_context.binding.clone();
    assert_eq!(
        run(&device_context, std::slice::from_ref(&device_grant), None),
        Decision::Allowed
    );
    device_context.binding.device = Some("device-b".into());
    assert_ne!(
        run(&device_context, &[device_grant], None),
        Decision::Allowed
    );
    assert_eq!(
        application_decision_os_privilege(Decision::Allowed),
        OsPrivilege::NotGranted
    );
    assert_eq!(qualify_legacy(false, Decision::Allowed), Decision::Denied);
    assert_eq!(qualify_legacy(true, Decision::Denied), Decision::Denied);
}

#[test]
fn revocation_fence_serializes_fixture_decisions() {
    use std::sync::{Arc, Barrier, Mutex};
    let state = Arc::new(Mutex::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let state = Arc::clone(&state);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            *state.lock().unwrap() = true;
            barrier.wait();
        })
    };
    barrier.wait();
    let fence = state.lock().unwrap();
    let binding = Binding {
        host: "h".into(),
        owner: "o".into(),
        device: None,
        resource_type: "library".into(),
        resource_id: "l".into(),
        action: "read".into(),
    };
    let principal = Principal::Browser { id: "p".into() };
    let context = Context {
        principal: principal.clone(),
        authentication_method: "browser".into(),
        audience: "h".into(),
        binding: binding.clone(),
        revision: 1,
        principal_revoked: *fence,
        device_revoked: false,
        approval_required: false,
    };
    let grant = Assignment {
        principal,
        binding,
        role: "fixture".into(),
        revision: 1,
        revoked: false,
        expires_at: None,
        deny: false,
    };
    let vocabulary = vec![("fixture".into(), "library".into(), "read".into())];
    assert_eq!(
        evaluate(&context, 1, 1, &[grant], &vocabulary),
        Decision::StaleOrRevoked
    );
    drop(fence);
    writer.join().unwrap();
}
