use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn pane_exit_does_not_override_user_focus_or_create_a_workspace() {
    for mode in ["user-focused", "workspace-remains", "only-pool"] {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        let state = root.join("state/herdr-sessionizer");
        let bin = root.join("bin");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(state.join("workspace-history"), "closing\nolder\n").unwrap();
        let mut workspaces = vec![
            serde_json::json!({"workspace_id":"pool","label":"__herdr_shell_pool__","focused":mode!="user-focused"}),
        ];
        if mode != "only-pool" {
            workspaces
                .push(serde_json::json!({"workspace_id":"older","focused":mode=="user-focused"}));
        }
        if mode == "workspace-remains" {
            workspaces.push(serde_json::json!({"workspace_id":"closing","focused":false}));
        }
        fs::write(
            root.join("workspaces.json"),
            serde_json::json!({"result":{"workspaces":workspaces}}).to_string(),
        )
        .unwrap();
        let herdr = bin.join("herdr");
        fs::write(&herdr, "#!/bin/sh\ncase \"$1 $2\" in\n  'pane list') printf '%s\\n' '{\"result\":{\"panes\":[]}}' ;;\n  'workspace list') cat \"$FIXTURE/workspaces.json\" ;;\n  *) exit 1 ;;\nesac\n").unwrap();
        fs::set_permissions(&herdr, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
            .arg("after-pane-exit")
            .env("HOME", root).env("FIXTURE", root)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HERDR_PLUGIN_EVENT_JSON", r#"{"event":"pane_exited","data":{"type":"pane_exited","pane_id":"closing-pane","workspace_id":"closing"}}"#)
            .output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn pane_exit_returns_from_pool_to_the_most_recent_surviving_workspace() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(state.join("workspace-history"), "closing\nolder\noldest\n").unwrap();
    fs::write(root.join("workspaces.json"), r#"{"result":{"workspaces":[{"workspace_id":"closing","focused":true},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false},{"workspace_id":"older","focused":false},{"workspace_id":"oldest","focused":false}]}}"#).unwrap();
    let herdr = bin.join("herdr");
    fs::write(&herdr, r#"#!/bin/sh
case "$1 $2" in
  'pane list')
    if [ -f "$FIXTURE/pane-polled" ]; then
      printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":true},{"workspace_id":"oldest","focused":false},{"workspace_id":"older","focused":false}]}}' > "$FIXTURE/workspaces.json"
      printf '%s\n' '{"result":{"panes":[]}}'
    else
      touch "$FIXTURE/pane-polled"
      printf '%s\n' '{"result":{"panes":[{"pane_id":"closing-pane","workspace_id":"closing"}]}}'
    fi
    ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'workspace focus')
    [ "$3" = older ] || exit 1
    printf '%s\n' "$3" > "$FIXTURE/focused"
    printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false},{"workspace_id":"oldest","focused":false},{"workspace_id":"older","focused":true}]}}' > "$FIXTURE/workspaces.json"
    ;;
  *) exit 1 ;;
esac
"#).unwrap();
    fs::set_permissions(&herdr, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("after-pane-exit")
        .env("HOME", root).env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HERDR_PLUGIN_EVENT_JSON", r#"{"event":"pane_exited","data":{"type":"pane_exited","pane_id":"closing-pane","workspace_id":"closing"}}"#)
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("focused")).unwrap(), "older\n");
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "older\noldest\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("current-workspace")).unwrap(),
        "older\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        "oldest\n"
    );
}
