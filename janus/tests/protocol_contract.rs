//! M5 Task 5.2: protocol wire-format contracts (3.2/3.3/3.4). Pins the JSON
//! tag names + key fields so a daemon/client version skew is caught, and
//! verifies the 0.3.0 wire-compatibility of `Response::GuardVerdict.
//! cognitive_context` (omitted when `None` via `skip_serializing_if` - so a
//! 0.4.0 daemon's response still parses on a 0.3.0 client).

use janus::protocol::{Request, Response};
use uuid::Uuid;

#[test]
fn request_tags_are_snake_case() {
    // Contract 3.2: the Request discriminant is `type` in snake_case.
    assert_eq!(
        serde_json::to_string(&Request::Ping).unwrap(),
        r#"{"type":"ping"}"#
    );
    assert!(
        serde_json::to_string(&Request::Blueprints)
            .unwrap()
            .contains(r#""type":"blueprints""#)
    );
    assert!(
        serde_json::to_string(&Request::Progress { blueprint: None })
            .unwrap()
            .contains(r#""type":"progress""#)
    );
    assert!(
        serde_json::to_string(&Request::Onboard {
            name: "gatemetric".into()
        })
        .unwrap()
        .contains(r#""type":"onboard""#)
    );
    assert!(
        serde_json::to_string(&Request::Offboard {
            name: "gatemetric".into()
        })
        .unwrap()
        .contains(r#""type":"offboard""#)
    );
    let gc = Request::GuardCheck {
        execution_id: "e".into(),
        blueprint_id: None,
        task_id: None,
        step_name: None,
        cwd: None,
        argv: vec![],
        env_snapshot: Default::default(),
    };
    assert!(
        serde_json::to_string(&gc)
            .unwrap()
            .contains(r#""type":"guard_check""#)
    );
}

#[test]
fn guard_check_round_trips_with_all_fields() {
    // Contract 3.2: every GuardCheck field survives a serialize -> deserialize
    // round-trip (no field dropped/renamed on the wire).
    let req = Request::GuardCheck {
        execution_id: "exec-9".into(),
        blueprint_id: Some("gatemetric".into()),
        task_id: Some(Uuid::nil()),
        step_name: Some("cross-compile".into()),
        cwd: Some("/repo".into()),
        argv: vec!["make".into(), "flash".into()],
        env_snapshot: [("JANUS_AGENT".into(), "deployer".into())]
            .into_iter()
            .collect(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    match back {
        Request::GuardCheck {
            execution_id,
            blueprint_id,
            task_id,
            step_name,
            cwd,
            argv,
            env_snapshot,
        } => {
            assert_eq!(execution_id, "exec-9");
            assert_eq!(blueprint_id.as_deref(), Some("gatemetric"));
            assert_eq!(task_id, Some(Uuid::nil()));
            assert_eq!(step_name.as_deref(), Some("cross-compile"));
            assert_eq!(cwd.as_deref(), Some("/repo"));
            assert_eq!(argv, vec!["make".to_string(), "flash".to_string()]);
            assert_eq!(
                env_snapshot.get("JANUS_AGENT").map(|s| s.as_str()),
                Some("deployer")
            );
        }
        other => panic!("expected GuardCheck, got {other:?}"),
    }
}

#[test]
fn guard_verdict_cognitive_context_omitted_when_none() {
    // 0.3.0 wire compat (Contract 4.1): cognitive_context is
    // `skip_serializing_if = "Option::is_none"`, so a 0.4.0 daemon response
    // with no cognitive reason omits the field entirely -> a 0.3.0 client
    // (which doesn't know the field) can still deserialize it.
    let resp = Response::GuardVerdict {
        execution_id: "e".into(),
        verdict: "ALLOW".into(),
        reason: None,
        rewritten_argv: None,
        correlation_id: "c".into(),
        cognitive_context: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(
        !json.contains("cognitive_context"),
        "None cognitive_context should be omitted for 0.3.0 wire compat: {json}"
    );
    // And it still parses.
    let _: Response = serde_json::from_str(&json).unwrap();
}

#[test]
fn guard_verdict_cognitive_context_included_when_some() {
    // Contract 4.1: a cognitive BLOCK reason is carried on the wire.
    let resp = Response::GuardVerdict {
        execution_id: "e".into(),
        verdict: "BLOCK".into(),
        reason: Some("cognitive".into()),
        rewritten_argv: None,
        correlation_id: "c".into(),
        cognitive_context: Some("pin conflict: GPIO 21 I2C".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains(r#""cognitive_context":"pin conflict: GPIO 21 I2C""#));
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::GuardVerdict {
            cognitive_context, ..
        } => assert_eq!(
            cognitive_context.as_deref(),
            Some("pin conflict: GPIO 21 I2C")
        ),
        other => panic!("expected GuardVerdict, got {other:?}"),
    }
}

#[test]
fn response_tags_are_snake_case() {
    // Contract 3.3/3.4/Ok/Error: response discriminants.
    assert_eq!(
        serde_json::to_string(&Response::Pong).unwrap(),
        r#"{"type":"pong"}"#
    );
    assert!(
        serde_json::to_string(&Response::Ok {
            message: "m".into()
        })
        .unwrap()
        .contains(r#""type":"ok""#)
    );
    assert!(
        serde_json::to_string(&Response::Error {
            message: "m".into()
        })
        .unwrap()
        .contains(r#""type":"error""#)
    );
    assert!(
        serde_json::to_string(&Response::Blueprints { blueprints: vec![] })
            .unwrap()
            .contains(r#""type":"blueprints""#)
    );
    assert!(
        serde_json::to_string(&Response::Progress {
            active_tasks: vec![]
        })
        .unwrap()
        .contains(r#""type":"progress""#)
    );
}
