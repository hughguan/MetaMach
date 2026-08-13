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
    assert!(
        serde_json::to_string(&Request::Dispatch {
            blueprint: "demo".into(),
            workflow: Some("req2spec".into()),
            inline_command: None,
        })
        .unwrap()
        .contains(r#""type":"dispatch""#)
    );
    assert!(
        serde_json::to_string(&Request::Stop {
            blueprint: Some("demo".into()),
            task_id: None,
        })
        .unwrap()
        .contains(r#""type":"stop""#)
    );
    assert!(
        serde_json::to_string(&Request::Continue {
            blueprint: Some("demo".into()),
            task_id: None,
        })
        .unwrap()
        .contains(r#""type":"continue""#)
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

#[test]
fn utc_34_01_typed_checkpoint_envelope_wire_contract() {
    use janus::workflow::{BuildEnvelope, CheckpointEnvelope, EnvelopeBase, TypedEnvelope};
    use serde_json::json;

    let task_id = Uuid::new_v4();
    let env = CheckpointEnvelope::new(
        task_id,
        "builder_implement",
        "COMPLETED",
        Some(0),
        None,
        json!({"exit": 0}),
    );

    let val = env
        .to_checkpoint_value()
        .expect("valid envelope serialization");
    assert_eq!(val["version"], 1);
    assert_eq!(val["step_name"], "builder_implement");
    assert_eq!(val["status"], "COMPLETED");

    let parsed = CheckpointEnvelope::parse_checkpoint(&val, task_id);
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.step_name, "builder_implement");

    // Test legacy fallback decoder
    let legacy_json = json!({
        "step": "architect_design",
        "status": "SUSPENDED",
        "hitl_verdict": "APPROVED"
    });
    let legacy_parsed = CheckpointEnvelope::parse_checkpoint(&legacy_json, task_id);
    assert_eq!(legacy_parsed.version, 0);
    assert_eq!(legacy_parsed.step_name, "architect_design");
    assert_eq!(legacy_parsed.status, "SUSPENDED");
    assert_eq!(legacy_parsed.hitl_verdict.as_deref(), Some("APPROVED"));

    // Domain-specific envelope test
    let build_env = BuildEnvelope {
        base: EnvelopeBase {
            status: "COMPLETED".to_string(),
            summary: "Built binaries".to_string(),
            artifacts: vec!["target/release/janus".to_string()],
            notes_for_next_agent: "Run tests".to_string(),
        },
        changed_files: vec!["src/lib.rs".to_string()],
        commit_hash: Some("16f62a4".to_string()),
    };
    assert!(build_env.validate().is_ok());
}

#[tokio::test]
async fn utc_36_01_credential_provider_spi_contract() {
    use janus::credential::{CredentialProvider, MemoryCredentialProvider, NoopCredentialProvider};

    let task_id = Uuid::new_v4();

    // Noop provider contract
    let noop = NoopCredentialProvider;
    let cred = noop
        .provision(task_id, &["read".into()], 300)
        .await
        .unwrap();
    assert_eq!(cred.task_id, task_id);
    assert!(cred.key.starts_with("noop-key-"));
    assert!(noop.revoke(task_id).await.is_ok());

    // Memory provider contract & sweep
    let mem = MemoryCredentialProvider::new();
    let cred_mem = mem
        .provision(task_id, &["read".into(), "write".into()], 600)
        .await
        .unwrap();
    assert_eq!(cred_mem.task_id, task_id);
    assert_eq!(mem.active_count(), 1);

    // Reconcile / sweep active tasks
    let swept = mem.cleanup_sweep(&[task_id]).await.unwrap();
    assert_eq!(swept, 0); // Still active

    let swept_dead = mem.cleanup_sweep(&[]).await.unwrap();
    assert_eq!(swept_dead, 1); // Revoked
    assert_eq!(mem.active_count(), 0);
}

#[tokio::test]
async fn utc_33_01_dual_track_execution_writes_guard_contract() {
    use janus::recipe::WorkflowStep;
    use janus::workflow::verify_post_execution_writes;

    let toml_str = r#"
        name = "build_web_ui"
        agent = "builder"
        command = "bun install"
        isolation = "sandbox"
        writes = ["apps/web/dist/", "apps/web/package.json"]
    "#;
    let step: WorkflowStep = toml::from_str(toml_str).unwrap();
    assert_eq!(step.name, "build_web_ui");
    assert_eq!(step.isolation.as_deref(), Some("sandbox"));
    assert_eq!(
        step.writes.as_deref(),
        Some(
            &[
                "apps/web/dist/".to_string(),
                "apps/web/package.json".to_string()
            ][..]
        )
    );

    let tmp = tempfile::tempdir().unwrap();
    let task_id = Uuid::new_v4();

    // Verification on clean / non-git workspace passes
    let ok =
        verify_post_execution_writes(tmp.path(), task_id, "build_web_ui", step.writes.as_deref())
            .unwrap();
    assert!(ok);

    // Initialize git repo and test unauthorized modification snapshot
    let repo_dir = tmp.path();
    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "MetaMach Test"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@metamach.internal"])
        .current_dir(repo_dir)
        .output();

    std::fs::create_dir_all(repo_dir.join("apps/web")).unwrap();
    std::fs::write(repo_dir.join("apps/web/package.json"), "{}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir)
        .output();

    // Create unauthorized write
    std::fs::write(repo_dir.join("unauthorized.txt"), "forbidden content").unwrap();

    let guard_passed =
        verify_post_execution_writes(repo_dir, task_id, "build_web_ui", step.writes.as_deref())
            .unwrap();
    assert!(!guard_passed);

    // Verify recovery ref snapshot commit was created and holds unauthorized file
    let ref_name = format!("refs/metamach/rollback/{}-build_web_ui", task_id.simple());
    let rev_out = std::process::Command::new("git")
        .args(["rev-parse", &ref_name])
        .current_dir(repo_dir)
        .output()
        .unwrap();
    assert!(rev_out.status.success());
    let ref_commit = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();
    assert!(!ref_commit.is_empty());

    let show_out = std::process::Command::new("git")
        .args(["show", &format!("{ref_name}:unauthorized.txt")])
        .current_dir(repo_dir)
        .output()
        .unwrap();
    assert!(show_out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&show_out.stdout).trim(),
        "forbidden content"
    );

    // Test tracked file modification in primary git stash create path
    let _ = std::process::Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["reset", "--hard"])
        .current_dir(repo_dir)
        .output();

    std::fs::write(repo_dir.join("README.md"), "initial").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "add readme"])
        .current_dir(repo_dir)
        .output();

    // Modify tracked file (not in writes whitelist)
    std::fs::write(repo_dir.join("README.md"), "unauthorized change").unwrap();

    let tracked_guard_passed =
        verify_post_execution_writes(repo_dir, task_id, "build_web_ui", step.writes.as_deref())
            .unwrap();
    assert!(!tracked_guard_passed);

    // Verify recovery ref captured the tracked modification
    let show_tracked = std::process::Command::new("git")
        .args(["show", &format!("{ref_name}:README.md")])
        .current_dir(repo_dir)
        .output()
        .unwrap();
    assert!(show_tracked.status.success());
    assert_eq!(
        String::from_utf8_lossy(&show_tracked.stdout).trim(),
        "unauthorized change"
    );

    // Verify prefix boundary matching: package.json.bak must NOT match package.json pattern
    let _ = std::process::Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["reset", "--hard"])
        .current_dir(repo_dir)
        .output();

    std::fs::write(repo_dir.join("apps/web/package.json.bak"), "backup").unwrap();
    let boundary_guard_passed =
        verify_post_execution_writes(repo_dir, task_id, "build_web_ui", step.writes.as_deref())
            .unwrap();
    assert!(!boundary_guard_passed);

    // Test mixed tracked modification + untracked file (git stash create -u path)
    let _ = std::process::Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["reset", "--hard"])
        .current_dir(repo_dir)
        .output();

    std::fs::write(repo_dir.join("README.md"), "mixed tracked change").unwrap();
    std::fs::write(
        repo_dir.join("untracked_mixed.txt"),
        "mixed untracked change",
    )
    .unwrap();

    let mixed_guard_passed =
        verify_post_execution_writes(repo_dir, task_id, "build_web_ui", step.writes.as_deref())
            .unwrap();
    assert!(!mixed_guard_passed);

    let show_mixed_tracked = std::process::Command::new("git")
        .args(["show", &format!("{ref_name}:README.md")])
        .current_dir(repo_dir)
        .output()
        .unwrap();
    assert!(show_mixed_tracked.status.success());
    assert_eq!(
        String::from_utf8_lossy(&show_mixed_tracked.stdout).trim(),
        "mixed tracked change"
    );

    let show_mixed_untracked = std::process::Command::new("git")
        .args(["show", &format!("{ref_name}:untracked_mixed.txt")])
        .current_dir(repo_dir)
        .output()
        .unwrap();
    assert!(show_mixed_untracked.status.success());
    assert_eq!(
        String::from_utf8_lossy(&show_mixed_untracked.stdout).trim(),
        "mixed untracked change"
    );
}

#[tokio::test]
async fn utc_36_02_herdr_harvest_pipeline_contract() {
    use janus::harvest::{apply_harvest_ref, harvest_sandbox_output, list_harvest_refs};

    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path();
    let task_id = Uuid::new_v4();

    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "MetaMach Test"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@metamach.internal"])
        .current_dir(repo_dir)
        .output();

    std::fs::write(repo_dir.join("README.md"), "# Initial").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir)
        .output();

    // Create sandbox output changes in working directory
    std::fs::write(repo_dir.join("harvest_artifact.txt"), "harvested content").unwrap();

    // Harvest output
    let ref_name = harvest_sandbox_output(repo_dir, task_id, "build_step").unwrap();
    assert_eq!(
        ref_name,
        format!("refs/sandbox/{}-build_step", task_id.simple())
    );

    // Verify list_harvest_refs includes the harvest ref
    let refs = list_harvest_refs(repo_dir).unwrap();
    assert!(refs.contains(&ref_name));

    // Reset workspace to clean HEAD
    let _ = std::process::Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(repo_dir)
        .output();

    assert!(!repo_dir.join("harvest_artifact.txt").exists());

    // Apply harvest ref into working tree
    apply_harvest_ref(repo_dir, task_id, "build_step").unwrap();
    assert!(repo_dir.join("harvest_artifact.txt").exists());
    assert_eq!(
        std::fs::read_to_string(repo_dir.join("harvest_artifact.txt")).unwrap(),
        "harvested content"
    );
}

#[test]
fn utc_33_02_sandboxed_tmux_worktree_contract() {
    use janus::harvest::{cleanup_sandbox_worktree, create_sandbox_worktree};
    use janus::tmux::TmuxBackend;

    let temp_dir = tempfile::tempdir().unwrap();
    let repo_dir = temp_dir.path();

    // Initialize git repository
    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .output()
        .unwrap();

    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "MetaMach Test"])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@metamach.local"])
        .current_dir(repo_dir)
        .output();

    std::fs::write(repo_dir.join("main.txt"), "main branch content").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo_dir)
        .output();

    let task_id = Uuid::new_v4();
    let step_name = "sandbox_build";
    let worktree_dir = repo_dir.join(format!(".janus/sandboxes/{}-{step_name}", task_id.simple()));

    // 1. Create sandbox worktree
    create_sandbox_worktree(repo_dir, &worktree_dir, task_id, step_name).unwrap();
    assert!(worktree_dir.exists());
    assert!(worktree_dir.join("main.txt").exists());

    // 2. Perform write inside sandbox worktree (isolated from main workspace)
    std::fs::write(
        worktree_dir.join("sandbox_artifact.txt"),
        "sandbox build output",
    )
    .unwrap();
    assert!(worktree_dir.join("sandbox_artifact.txt").exists());
    assert!(!repo_dir.join("sandbox_artifact.txt").exists());

    // 3. Verify isolated TmuxBackend constructor
    let sandbox_backend =
        TmuxBackend::with_socket(format!("metamach-sandbox-{}", task_id.simple()));
    assert!(!sandbox_backend.is_remote());

    // 4. Clean up sandbox worktree
    cleanup_sandbox_worktree(repo_dir, &worktree_dir, task_id, step_name).unwrap();
    assert!(!worktree_dir.exists());
    assert!(!repo_dir.join("sandbox_artifact.txt").exists());
}
