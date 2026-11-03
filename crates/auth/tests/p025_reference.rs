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
    assert_eq!(check(&c, &[a.clone()]), Decision::Allowed);
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
        assert_ne!(check(&altered, &[a.clone()]), Decision::Allowed);
    }
    let mut deny = a.clone();
    deny.deny = true;
    assert_eq!(check(&c, &[a.clone(), deny]), Decision::Denied);
    let mut revoked = a.clone();
    revoked.revoked = true;
    assert_eq!(check(&c, &[revoked]), Decision::StaleOrRevoked);
    assert_eq!(
        evaluate(&c, 1, 10, &[a.clone()], &vocabulary),
        Decision::StaleOrRevoked
    );
    c.approval_required = true;
    assert_eq!(check(&c, &[a.clone()]), Decision::RequiresApproval);
    c.principal_revoked = true;
    assert_eq!(check(&c, &[a]), Decision::StaleOrRevoked);
}
