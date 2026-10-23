use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn script(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn standalone_sessionizer_creates_focuses_and_jumps_back_without_shell_pool() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path();
    let home = root.join("home");
    let old_project = home.join("devel/old project");
    let new_project = home.join("devel/new project");
    let bin = root.join("bin");
    let plugin = root.join("plugins/herdr-sessionizer");
    let state = root.join("state/herdr-sessionizer");
    for path in [
        &old_project,
        &new_project,
        &home.join("VAULT"),
        &bin,
        &plugin,
        &state,
    ] {
        fs::create_dir_all(path).unwrap();
    }
    // Mirror the deployed layout but deliberately do not install the sibling tool.
    let binary = plugin.join("herdr-sessionizer");
    fs::copy(env!("CARGO_BIN_EXE_herdr-sessionizer"), &binary).unwrap();
    assert!(!root.join("plugins/herdr-shell-pool").exists());
    fs::write(state.join("workspace-history"), "pool\nold\n").unwrap();
    fs::write(root.join("workspaces.json"), r#"{"result":{"workspaces":[{"workspace_id":"old","label":"old project","focused":false},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":true}]}}"#).unwrap();
    script(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'workspace create')
    printf '%s\n' "$@" >> "$FIXTURE/actions"
    printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"old","label":"old project","focused":false},{"workspace_id":"new","label":"new project","focused":true},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false}]}}' > "$FIXTURE/workspaces.json"
    printf '%s\n' '{"result":{"tab":{"tab_id":"new-tab"}}}'
    ;;
  'tab rename') printf '%s\n' "$@" >> "$FIXTURE/actions" ;;
  'workspace focus')
    printf '%s\n' "$@" >> "$FIXTURE/actions"
    case "$3" in
      old) old=true; new=false ;;
      new) old=false; new=true ;;
      *) exit 1 ;;
    esac
    printf '{"result":{"workspaces":[{"workspace_id":"old","label":"old project","focused":%s},{"workspace_id":"new","label":"new project","focused":%s},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false}]}}\n' "$old" "$new" > "$FIXTURE/workspaces.json"
    ;;
  *) exit 1 ;;
esac
"#,
    );
    script(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' \"$PICK_PATH\"\n",
    );
    let run = |action: &str, selection: &Path| {
        let output = Command::new(&binary)
            .arg(action)
            .env("HOME", &home)
            .env("FIXTURE", root)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("PICK_PATH", selection)
            .env_remove("HERDR_SESSIONIZER_PANE_ID")
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{action}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    let created = run("pick", &new_project);
    assert_eq!(
        String::from_utf8(created.stdout).unwrap(),
        format!("{}\n", new_project.display())
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "new\nold\n"
    );
    let creation = format!(
        "workspace\ncreate\n--cwd\n{}\n--label\nnew project\n--focus\ntab\nrename\nnew-tab\nnew project\n",
        new_project.display()
    );
    assert_eq!(fs::read_to_string(root.join("actions")).unwrap(), creation);

    let focused = run("pick", &old_project);
    assert_eq!(
        String::from_utf8(focused.stdout).unwrap(),
        format!("{}\n", old_project.display())
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "old\nnew\n"
    );
    let jumped = run("jump-back", &old_project);
    assert!(jumped.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(root.join("actions")).unwrap(),
        format!("{creation}workspace\nfocus\nold\nworkspace\nfocus\nnew\n")
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "new\nold\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("current-workspace")).unwrap(),
        "new\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        "old\n"
    );
}

#[test]
fn picker_delegates_new_workspace_and_preserves_directory_output_and_history() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path();
    let home = root.join("home");
    let project = home.join("devel/new project");
    let bin = root.join("bin");
    let plugin = root.join("plugins/herdr-sessionizer");
    let pool = root.join("plugins/herdr-shell-pool");
    for path in [&project, &home.join("VAULT"), &bin, &plugin, &pool] {
        fs::create_dir_all(path).unwrap();
    }
    let binary = plugin.join("herdr-sessionizer");
    fs::copy(env!("CARGO_BIN_EXE_herdr-sessionizer"), &binary).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"old","label":"other","focused":true}]}}"#,
    )
    .unwrap();
    script(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  *) printf 'unexpected ordinary Herdr action: %s %s\n' "$1" "$2" >&2; exit 1 ;;
esac
"#,
    );
    script(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' \"$HOME/devel/new project\"\n",
    );
    script(
        &pool.join("herdr-shell-pool-bin"),
        r#"#!/bin/sh
printf '%s\n' "$@" > "$FIXTURE/pool-arguments"
printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"old","label":"other","focused":false},{"workspace_id":"new","label":"new project","focused":true}]}}' > "$FIXTURE/workspaces.json"
# Tool output must not corrupt sessionizer's selected-directory output.
printf 'pool diagnostic output\n'
"#,
    );

    let output = Command::new(binary)
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env_remove("HERDR_SESSIONIZER_PANE_ID")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = project;
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", directory.display())
    );
    assert_eq!(
        fs::read_to_string(root.join("pool-arguments")).unwrap(),
        format!(
            "new-workspace\n--cwd\n{}\n--label\nnew project\n",
            directory.display()
        )
    );
    assert_eq!(
        fs::read_to_string(root.join("state/herdr-sessionizer/workspace-history")).unwrap(),
        "new\nold\n"
    );
}

#[test]
fn pool_delegate_failure_is_reported_without_ordinary_create_retry_or_stdout() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path();
    let home = root.join("home");
    let project = home.join("devel/new-project");
    let bin = root.join("bin");
    let plugin = root.join("plugins/herdr-sessionizer");
    let pool = root.join("plugins/herdr-shell-pool");
    for path in [&project, &home.join("VAULT"), &bin, &plugin, &pool] {
        fs::create_dir_all(path).unwrap();
    }
    let binary = plugin.join("herdr-sessionizer");
    fs::copy(env!("CARGO_BIN_EXE_herdr-sessionizer"), &binary).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[]}}"#,
    )
    .unwrap();
    script(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'workspace create') echo ordinary-create >> "$FIXTURE/actions"; exit 1 ;;
  *) exit 1 ;;
esac
"#,
    );
    script(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' \"$HOME/devel/new-project\"\n",
    );
    script(
        &pool.join("herdr-shell-pool-bin"),
        r#"#!/bin/sh
count=$(cat "$FIXTURE/pool-count" 2>/dev/null || printf 0)
count=$((count + 1))
printf '%s\n' "$count" > "$FIXTURE/pool-count"
printf 'pool failed\n' >&2
exit 1
"#,
    );

    let output = Command::new(binary)
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env_remove("HERDR_SESSIONIZER_PANE_ID")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no creation retry was attempted"));
    assert_eq!(fs::read_to_string(root.join("pool-count")).unwrap(), "1\n");
    assert!(!root.join("actions").exists());
}
