use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn run(binary: &str, command: &str, root: &Path, bin: &Path) -> Output {
    Command::new(binary)
        .arg(command)
        .env("HOME", root)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap()
}

#[test]
fn track_excludes_pool_from_focused_saved_and_legacy_history() {
    for mode in ["saved", "legacy", "only-pool"] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let state = root.join("state/herdr-sessionizer");
        let bin = root.join("bin");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let mut workspaces = vec![
            serde_json::json!({"workspace_id":"pool", "label":"__herdr_shell_pool__", "focused":true}),
        ];
        if mode != "only-pool" {
            workspaces.extend([
                serde_json::json!({"workspace_id":"wA", "focused":false}),
                serde_json::json!({"workspace_id":"wB", "focused":false}),
            ]);
        }
        fs::write(
            root.join("workspaces.json"),
            serde_json::json!({"result":{"workspaces":workspaces}}).to_string(),
        )
        .unwrap();
        executable(
            &bin.join("herdr"),
            "#!/bin/sh\ncat \"$FIXTURE/workspaces.json\"\n",
        );
        if mode == "legacy" {
            fs::write(state.join("current-workspace"), "pool\n").unwrap();
            fs::write(state.join("previous-workspace"), "wA\n").unwrap();
        } else {
            fs::write(state.join("workspace-history"), "pool\nwA\npool\nwB\n").unwrap();
            fs::write(state.join("current-workspace"), "pool\n").unwrap();
            fs::write(state.join("previous-workspace"), "pool\n").unwrap();
        }
        let output = run(env!("CARGO_BIN_EXE_herdr-sessionizer"), "track", root, &bin);
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (history, current, previous) = match mode {
            "saved" => ("wA\nwB\n", "wA\n", "wB\n"),
            "legacy" => ("wA\n", "wA\n", ""),
            _ => ("\n", "", ""),
        };
        assert_eq!(
            fs::read_to_string(state.join("workspace-history")).unwrap(),
            history,
            "{mode}"
        );
        assert_eq!(
            fs::read_to_string(state.join("current-workspace")).unwrap(),
            current,
            "{mode}"
        );
        assert_eq!(
            fs::read_to_string(state.join("previous-workspace")).unwrap(),
            previous,
            "{mode}"
        );
    }
}

#[test]
fn jump_back_skips_pool_entries_already_in_saved_history() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\npool\nwB\n").unwrap();
    fs::write(root.join("workspaces.json"), r#"{"result":{"workspaces":[{"workspace_id":"wA","focused":true},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false},{"workspace_id":"wB","focused":false}]}}"#).unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'workspace focus')
    [ "$3" = wB ] || exit 1
    printf '%s\n' "$3" > "$FIXTURE/focused"
    printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"wA","focused":false},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false},{"workspace_id":"wB","focused":true}]}}' > "$FIXTURE/workspaces.json"
    ;;
  *) exit 1 ;;
esac
"#,
    );
    let output = run(
        env!("CARGO_BIN_EXE_herdr-sessionizer"),
        "jump-back",
        root,
        &bin,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("focused")).unwrap(), "wB\n");
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "wB\nwA\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("current-workspace")).unwrap(),
        "wB\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        "wA\n"
    );
}

#[test]
fn track_promotes_focused_workspace_and_deduplicates_open_history() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();

    executable(
        &bin.join("herdr"),
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[{\"workspace_id\":\"wCurrent\",\"focused\":true},{\"workspace_id\":\"wOlder\",\"focused\":false}]}}'\n",
    );
    fs::write(
        state.join("workspace-history"),
        "closed\nwOlder\nwOlder\nwCurrent\n",
    )
    .unwrap();

    let output = run(env!("CARGO_BIN_EXE_herdr-sessionizer"), "track", root, &bin);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "wCurrent\nwOlder\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("current-workspace")).unwrap(),
        "wCurrent\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        "wOlder\n"
    );
}

#[test]
fn picker_discovers_only_supported_depths_and_prunes_development_archives() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    let depth_one = home.join("VAULT/one");
    let depth_two = depth_one.join("two");
    let depth_three = depth_two.join("three");
    let depth_four = depth_three.join("four");
    fs::create_dir_all(&depth_four).unwrap();
    fs::create_dir_all(home.join("VAULT")).unwrap();
    fs::create_dir_all(home.join("devel/Archive/hidden")).unwrap();
    fs::create_dir_all(home.join("devel/.sync/hidden")).unwrap();
    fs::create_dir_all(home.join("devel/.SynologyWorkingDirectory/hidden")).unwrap();
    fs::create_dir_all(home.join("devel/visible")).unwrap();
    fs::create_dir_all(home.join("outside/target/nested")).unwrap();
    fs::write(home.join("outside/file"), "not a directory").unwrap();
    symlink(home.join("outside/target"), home.join("devel/link")).unwrap();
    symlink(home.join("outside/file"), home.join("devel/file-link")).unwrap();
    symlink(home.join("outside/missing"), home.join("devel/broken-link")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
exit 0
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    for path in [
        &depth_one,
        &depth_two,
        &depth_three,
        &home.join("devel/link"),
    ] {
        assert!(candidates.contains(&format!("{}\t", path.display())));
    }
    for path in [
        &depth_four,
        &home.join("devel/Archive"),
        &home.join("devel/Archive/hidden"),
        &home.join("devel/.sync/hidden"),
        &home.join("devel/.SynologyWorkingDirectory/hidden"),
        &home.join("outside/target"),
        &home.join("outside/target/nested"),
        &home.join("devel/file-link"),
        &home.join("devel/broken-link"),
    ] {
        assert!(!candidates.contains(&format!("{}\t", path.display())));
    }
}

#[test]
fn picker_uses_workspace_labels_before_the_first_pane_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT")).unwrap();
    fs::create_dir_all(home.join("devel/EKEFE")).unwrap();
    fs::create_dir_all(home.join("dotfiles/.local/bin")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"w1","label":"EKEFE","focused":true},{"workspace_id":"w2","label":"other","focused":false},{"workspace_id":"w3","label":"dotfiles","focused":false}]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("panes.json"),
        format!(
            r#"{{"result":{{"panes":[{{"workspace_id":"w1","cwd":"{}"}},{{"workspace_id":"w2","cwd":"{}"}},{{"workspace_id":"w3","cwd":"{}"}}]}}}}"#,
            home.join("devel").display(),
            home.join("other").display(),
            home.join("dotfiles/.local/bin").display()
        ),
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') cat "$FIXTURE/panes.json" ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
exit 0
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    let (open, _) = candidates.split_once("\n\n").unwrap();
    assert!(open.contains(&home.join("devel/EKEFE").display().to_string()));
    assert!(!open.contains(&format!("{}\t", home.join("devel").display())));
    assert!(open.contains(&home.join("other").display().to_string()));
    assert!(open.contains(&format!("{}\t", home.join("dotfiles").display())));
    assert!(!open.contains(&home.join("dotfiles/.local/bin").display().to_string()));
}

#[test]
fn picker_shows_agent_counts_and_handles_empty_successful_selection() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/wA")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"wA","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("agents.json"),
        r#"{"result":{"agents":[{"workspace_id":"wA","agent":"pi"},{"workspace_id":"wA","agent":"pi"},{"workspace_id":"wA","agent":"codex"}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  'pane close') printf '%s\n' "$3" > "$FIXTURE/closed-pane" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
exit 0
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HERDR_SESSIONIZER_PANE_ID", "picker-pane")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("~/VAULT/wA"));
    assert!(candidates.contains("pi"));
    assert!(candidates.contains("codex"));
    assert!(!candidates.contains("Codex: 1, Pi: 2"));
    assert_eq!(
        fs::read_to_string(root.join("closed-pane")).unwrap(),
        "picker-pane\n"
    );
    assert!(!root.join("actions").exists());
}

#[test]
fn track_imports_legacy_current_and_previous_state_when_history_is_absent() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("current-workspace"), "wCurrent\n").unwrap();
    fs::write(state.join("previous-workspace"), "wPrevious\n").unwrap();
    executable(
        &bin.join("herdr"),
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[{\"workspace_id\":\"wCurrent\",\"focused\":true},{\"workspace_id\":\"wPrevious\",\"focused\":false}]}}'\n",
    );

    let output = run(env!("CARGO_BIN_EXE_herdr-sessionizer"), "track", root, &bin);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "wCurrent\nwPrevious\n"
    );
}

#[test]
fn picker_cleans_up_temporary_files_and_closes_its_pane_after_fzf_failure() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    let temp_root = root.join("tmp");
    fs::create_dir_all(home.join("VAULT")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&temp_root).unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') printf '%s\n' '{"result":{"workspaces":[]}}' ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'pane close') printf '%s\n' "$3" > "$FIXTURE/closed-pane" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
exit 1
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HERDR_SESSIONIZER_PANE_ID", "picker-pane")
        .env("TMPDIR", &temp_root)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(root.join("closed-pane")).unwrap(),
        "picker-pane\n"
    );
    assert_eq!(fs::read_dir(&temp_root).unwrap().count(), 0);
}

#[test]
fn tracker_uses_default_state_directory_when_xdg_state_home_is_empty() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("herdr"),
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[{\"workspace_id\":\"wA\",\"focused\":true}]}}'\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("track")
        .env("HOME", root)
        .env("XDG_STATE_HOME", "")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join(".local/state/herdr-sessionizer/workspace-history")).unwrap(),
        "wA\n"
    );
}

#[test]
fn existing_empty_history_file_takes_precedence_over_legacy_projections() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "").unwrap();
    fs::write(state.join("current-workspace"), "wA\n").unwrap();
    fs::write(state.join("previous-workspace"), "wB\n").unwrap();
    executable(
        &bin.join("herdr"),
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[{\"workspace_id\":\"wA\",\"focused\":false},{\"workspace_id\":\"wB\",\"focused\":false}]}}'\n",
    );

    let output = run(env!("CARGO_BIN_EXE_herdr-sessionizer"), "track", root, &bin);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("workspace-history")).unwrap(),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("current-workspace")).unwrap(),
        ""
    );
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        ""
    );
}

#[test]
fn picker_lists_previous_workspace_first_and_focuses_the_selection() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/wA")).unwrap();
    fs::create_dir_all(home.join("VAULT/wB")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wB\nwA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"wA","focused":true},{"workspace_id":"wB","label":"wB","focused":false}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'workspace focus') printf '%s\n' "$3" >> "$FIXTURE/actions" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
head -n 1 "$FIXTURE/candidates"
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        home.join("VAULT/wB").display().to_string()
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    let (open, unopened) = candidates.split_once("\n\n").unwrap();
    assert_eq!(
        open.lines()
            .map(|line| line.split('\t').next().unwrap())
            .collect::<Vec<_>>(),
        vec![
            home.join("VAULT/wB").to_str().unwrap(),
            home.join("VAULT/wA").to_str().unwrap()
        ]
    );
    assert_eq!(fs::read_to_string(root.join("actions")).unwrap(), "wB\n");
    assert!(unopened.contains(&home.join("devel").display().to_string()));
}

#[test]
fn picker_creates_first_workspace_when_only_the_pool_is_open() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let project = home.join("devel/first-project");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(home.join("VAULT")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":true}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'workspace create') printf '%s\n' "$@" > "$FIXTURE/actions"; printf '%s\n' '{"result":{"tab":{"tab_id":"tab-1"}}}' ;;
  'tab rename') printf '%s\n' "$@" >> "$FIXTURE/actions" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
awk -F '\t' -v target="$PICK_PATH" '$1 == target { print; exit }' "$FIXTURE/candidates"
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("PICK_PATH", &project)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        project.display().to_string()
    );
    assert_eq!(
        fs::read_to_string(root.join("actions")).unwrap(),
        format!(
            "workspace\ncreate\n--cwd\n{}\n--label\nfirst-project\n--focus\ntab\nrename\ntab-1\nfirst-project\n",
            project.display()
        ),
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.starts_with('\n'));
    assert!(!candidates.contains("__herdr_shell_pool__"));
}

#[test]
fn jump_back_does_not_retry_when_previous_workspace_closes_before_focus() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        state.join("workspace-history"),
        "wCurrent\nwPrevious\nwOlder\n",
    )
    .unwrap();
    fs::write(root.join("list-count"), "0\n").unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'workspace list')
    count=$(cat "$FIXTURE/list-count")
    count=$((count + 1))
    printf '%s\n' "$count" > "$FIXTURE/list-count"
    if [ "$count" -eq 1 ]; then
      printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"wCurrent","focused":true},{"workspace_id":"wPrevious","focused":false},{"workspace_id":"wOlder","focused":false}]}}'
    else
      printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"wCurrent","focused":true}]}}'
    fi
    ;;
  'workspace focus') echo unexpected-focus >> "$FIXTURE/actions"; exit 1 ;;
  *) exit 1 ;;
esac
"###,
    );

    let output = run(
        env!("CARGO_BIN_EXE_herdr-sessionizer"),
        "jump-back",
        root,
        &bin,
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join("actions").exists());
    assert!(!state.join("previous-workspace").exists());
}

#[test]
fn jump_back_is_a_successful_noop_with_zero_or_one_open_workspace() {
    for workspaces in [
        r#"{"result":{"workspaces":[]}}"#,
        r#"{"result":{"workspaces":[{"workspace_id":"wA","focused":true}]}}"#,
    ] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let state = root.join("state/herdr-sessionizer");
        let bin = root.join("bin");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(state.join("workspace-history"), "wA\nwB\n").unwrap();
        fs::write(root.join("workspaces.json"), workspaces).unwrap();
        executable(
            &bin.join("herdr"),
            r###"#!/bin/sh
case "$1 $2" in
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'workspace focus') echo unexpected-focus >> "$FIXTURE/actions"; exit 1 ;;
  *) exit 1 ;;
esac
"###,
        );

        let output = run(
            env!("CARGO_BIN_EXE_herdr-sessionizer"),
            "jump-back",
            root,
            &bin,
        );

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.join("actions").exists());
        assert_eq!(
            fs::read_to_string(state.join("previous-workspace")).unwrap_or_default(),
            ""
        );
    }
}

#[test]
fn failed_jump_back_focus_keeps_the_previous_workspace_target() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wB\nwA\n").unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"wA","focused":false},{"workspace_id":"wB","focused":true}]}}' ;;
  'workspace focus') exit 1 ;;
  *) exit 1 ;;
esac
"###,
    );

    let output = run(
        env!("CARGO_BIN_EXE_herdr-sessionizer"),
        "jump-back",
        root,
        &bin,
    );

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(state.join("previous-workspace")).unwrap(),
        "wA\n"
    );
}

#[test]
fn jump_back_alternates_between_the_two_most_recent_open_workspaces() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(root.join("focused"), "wB\n").unwrap();
    fs::write(state.join("workspace-history"), "wB\nwA\n").unwrap();
    executable(
        &bin.join("herdr"),
        "#!/bin/sh\ncase \"$1 $2\" in\n  'workspace list')\n    focused=$(cat \"$FIXTURE/focused\")\n    if [ \"$focused\" = wA ]; then a=true; b=false; else a=false; b=true; fi\n    printf '{\"result\":{\"workspaces\":[{\"workspace_id\":\"wA\",\"focused\":%s},{\"workspace_id\":\"wB\",\"focused\":%s}]}}\\n' \"$a\" \"$b\"\n    ;;\n  'workspace focus') printf '%s\\n' \"$3\" >\"$FIXTURE/focused\" ;;\n  *) exit 1 ;;\nesac\n",
    );

    let binary = env!("CARGO_BIN_EXE_herdr-sessionizer");
    let first = run(binary, "jump-back", root, &bin);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("focused")).unwrap(), "wA\n");
    let second = run(binary, "jump-back", root, &bin);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("focused")).unwrap(), "wB\n");
}

#[test]
fn picker_keeps_missing_agent_ids_visible_until_dismissed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(root.join("agents.json"), r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"","pane_id":"wA:p1","display_name":"review","agent":"codex"}]}}"#).unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  *) echo unexpected-action >> "$FIXTURE/actions"; exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
count=$(cat "$FIXTURE/fzf-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/fzf-count"
printf '%s\n' "$*" > "$FIXTURE/fzf-args-$count"
cat > "$FIXTURE/candidates-$count"
if [ "$count" -eq 1 ]; then sed -n '2p' "$FIXTURE/candidates-$count"; fi
"###,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(!root.join("actions").exists());
    assert!(
        fs::read_to_string(root.join("fzf-args-2"))
            .unwrap()
            .contains("agent has no tab identifier")
    );
}

#[test]
fn picker_retries_agent_after_pane_failure_without_rollback() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(root.join("agents.json"), r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"wA:t1","pane_id":"wA:p1","display_name":"review","agent":"codex"}]}}"#).unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  'tab focus') printf 'tab %s\n' "$3" >> "$FIXTURE/actions" ;;
  'agent focus') printf 'pane %s\n' "$3" >> "$FIXTURE/actions"; count=$(cat "$FIXTURE/pane-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/pane-count"; [ "$count" -gt 1 ] ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
count=$(cat "$FIXTURE/fzf-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/fzf-count"
cat > "$FIXTURE/candidates-$count"
sed -n '2p' "$FIXTURE/candidates-$count"
"###,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        format!("{}\n", home.join("VAULT/project").display()).as_bytes()
    );
    assert_eq!(
        fs::read_to_string(root.join("actions")).unwrap(),
        "tab wA:t1\npane wA:p1\ntab wA:t1\npane wA:p1\n"
    );
}

#[test]
fn picker_selects_nested_agent_by_captured_tab_and_pane_ids() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(root.join("agents.json"), r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"wA:t1","pane_id":"wA:p1","display_name":"review","agent":"codex","agent_status":"working"},{"workspace_id":"wA","tab_id":"wA:t2","pane_id":"wA:p2","display_name":"review","agent":"codex","agent_status":"blocked"},{"workspace_id":"wA","tab_id":"wA:t3","pane_id":"wA:p3","display_agent":"schema-name","agent":"hermes","agent_status":"idle"},{"workspace_id":"wA","tab_id":"wA:t4","pane_id":"wA:p4","name":"done-job","type":"pi","agent_status":"done"},{"workspace_id":"wA","tab_id":"wA:t5","pane_id":"wA:p5","display_name":"odd","agent":"pi","agent_status":"paused"},{"workspace_id":"wA","tab_id":"wA:t6","pane_id":"wA:p6","display_name":"missing","agent":"pi"}]}}"#).unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  'tab focus'|'agent focus') printf '%s %s\n' "$1 $2" "$3" >> "$FIXTURE/actions" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
cat > "$FIXTURE/candidates"
sed -n '2p' "$FIXTURE/candidates"
"###,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", home.join("VAULT/project").display())
    );
    assert_eq!(
        fs::read_to_string(root.join("actions")).unwrap(),
        "tab focus wA:t1\nagent focus wA:p1\n"
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("review (codex)"));
    assert!(candidates.contains("schema-name (hermes)"));
    assert!(candidates.contains("[working]"));
    assert!(candidates.contains("[blocked]"));
    assert!(candidates.contains("[idle]"));
    assert!(candidates.contains("[done]"));
    assert!(candidates.contains("[paused]"));
    assert!(candidates.contains("[unknown]"));
    assert!(candidates.contains("\u{1b}[33m[working]"));
    assert!(candidates.contains("\u{1b}[31m[blocked]"));
    assert!(candidates.contains("\u{1b}[39m[idle]"));
    assert!(candidates.contains("\u{1b}[32m[done]"));
    assert!(candidates.contains("\u{1b}[90m[paused]"));
    assert!(candidates.contains("\u{1b}[90m[unknown]"));
    for pane_id in ["wA:p1", "wA:p2"] {
        assert!(candidates.lines().any(|line| {
            line.split('\t')
                .nth(1)
                .is_some_and(|display| display.contains(pane_id))
        }));
    }
}

#[test]
fn picker_reload_keeps_workspaces_and_filters_agents_by_search_fields() {
    for (query, expected, unexpected) in [
        ("project", &["review", "build"][..], &["deploy"][..]),
        ("codex", &["build"][..], &["review", "deploy"][..]),
    ] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let home = root.join("home");
        let state = root.join("state/herdr-sessionizer");
        let bin = root.join("bin");
        fs::create_dir_all(home.join("VAULT/project")).unwrap();
        fs::create_dir_all(home.join("VAULT/other")).unwrap();
        fs::create_dir_all(home.join("devel")).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(state.join("workspace-history"), "project\nother\n").unwrap();
        fs::write(
            root.join("workspaces.json"),
            r#"{"result":{"workspaces":[{"workspace_id":"wProject","label":"project","focused":true},{"workspace_id":"wOther","label":"other","focused":false}]}}"#,
        )
        .unwrap();
        fs::write(
            root.join("agents.json"),
            r#"{"result":{"agents":[{"workspace_id":"wProject","tab_id":"t1","pane_id":"p1","display_name":"review","agent":"pi"},{"workspace_id":"wProject","tab_id":"t2","pane_id":"p2","display_name":"build","agent":"codex"},{"workspace_id":"wOther","tab_id":"t3","pane_id":"p3","display_name":"deploy","agent":"hermes"}]}}"#,
        )
        .unwrap();
        executable(
            &bin.join("herdr"),
            r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  'workspace focus') exit 0 ;;
  *) exit 1 ;;
esac
"###,
        );
        executable(
            &bin.join("fzf"),
            r###"#!/bin/bash
input=$(cat)
for arg in "$@"; do
  if [[ "$arg" == --filter=* ]]; then
    query=${arg#--filter=}
    awk -F '\t' -v query="$query" 'BEGIN { query=tolower(query) } tolower($4) ~ query' <<< "$input"
    exit 0
  fi
done
printf '%s\n' "$input" > "$FIXTURE/initial"
for arg in "$@"; do
  if [[ "$arg" == --bind=change:reload:* ]]; then
    command=${arg#--bind=change:reload:}
    command=${command//\{q\}/$QUERY}
    bash -c "$command" > "$FIXTURE/reloaded"
    head -n 1 "$FIXTURE/reloaded"
    exit 0
  fi
done
exit 0
"###,
        );

        let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
            .arg("pick")
            .env("HOME", &home)
            .env("FIXTURE", root)
            .env("QUERY", query)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{query}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reloaded = fs::read_to_string(root.join("reloaded")).unwrap();
        assert!(
            reloaded.contains("workspace|wProject"),
            "reloaded for {query}: {reloaded:?}"
        );
        assert!(reloaded.contains("workspace|wOther"));
        for label in expected {
            assert!(reloaded.contains(label), "{query} should include {label}");
        }
        for label in unexpected {
            assert!(!reloaded.contains(label), "{query} should omit {label}");
        }
    }
}

#[test]
fn picker_shows_fallback_agent_label_and_omitted_workspace_warning() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true},{"workspace_id":"wMissing","label":"missing","focused":false}]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("panes.json"),
        r#"{"result":{"panes":[{"workspace_id":"wMissing","cwd":null,"foreground_cwd":null}]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("agents.json"),
        r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"t1","pane_id":"p1"}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') cat "$FIXTURE/panes.json" ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FIXTURE/fzf-args\"\ncat > \"$FIXTURE/candidates\"\nexit 0\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let arguments = fs::read_to_string(root.join("fzf-args")).unwrap();
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("Agent (p1)"));
    assert!(arguments.contains("Hidden workspaces (directory unavailable): missing"));
}

#[test]
fn picker_orders_previous_current_remaining_history_then_unvisited_and_keeps_separator() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    for name in ["previous", "current", "visited", "unvisited"] {
        fs::create_dir_all(home.join("VAULT").join(name)).unwrap();
    }
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::write(state.join("workspace-history"), "previous\nvisited\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"current","label":"current","focused":true},{"workspace_id":"previous","label":"previous","focused":false},{"workspace_id":"visited","label":"visited","focused":false},{"workspace_id":"unvisited","label":"unvisited","focused":false},{"workspace_id":"pool","label":"__herdr_shell_pool__","focused":false}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list'|'agent list') printf '%s\n' '{"result":{"panes":[],"agents":[]}}' ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > \"$FIXTURE/candidates\"\nexit 0\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    let (open, unopened) = candidates.split_once("\n\n").unwrap();
    let labels = open
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .map(|display| display.trim().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![
            "~/VAULT/previous",
            "~/VAULT/current",
            "~/VAULT/visited",
            "~/VAULT/unvisited"
        ]
    );
    assert!(!unopened.is_empty());
    assert!(!candidates.contains("__herdr_shell_pool__"));
}

#[test]
fn picker_rejects_missing_pane_id_before_focus_and_does_not_retarget() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("agents.json"),
        r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"captured-tab","pane_id":""}]}}"#,
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  pane\ list) printf '%s\n' '{"result":{"panes":[]}}' ;;
  agent\ list) cat "$FIXTURE/agents.json" ;;
  tab\ focus|agent\ focus|workspace\ focus) printf '%s %s\n' "$1 $2" "$3" >> "$FIXTURE/actions"; exit 1 ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
count=$(cat "$FIXTURE/fzf-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/fzf-count"; printf '%s\n' "$*" > "$FIXTURE/fzf-args-$count"
cat > "$FIXTURE/candidates-$count"
if [ "$count" -eq 1 ]; then sed -n '2p' "$FIXTURE/candidates-$count"; fi
"###,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(!root.join("actions").exists());
    assert!(
        fs::read_to_string(root.join("fzf-args-2"))
            .unwrap()
            .contains("agent has no pane identifier")
    );
}

#[test]
fn picker_retries_the_captured_agent_after_metadata_changes_without_retargeting() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let state = root.join("state/herdr-sessionizer");
    let bin = root.join("bin");
    fs::create_dir_all(home.join("VAULT/project")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(state.join("workspace-history"), "wA\n").unwrap();
    fs::write(
        root.join("workspaces.json"),
        r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#,
    )
    .unwrap();
    fs::write(root.join("agents.json"), r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"captured-tab","pane_id":"captured-pane","display_name":"review","agent":"codex"}]}}"#).unwrap();
    executable(
        &bin.join("herdr"),
        r###"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  workspace\ list) cat "$FIXTURE/workspaces.json" ;;
  pane\ list) printf '%s\n' '{"result":{"panes":[]}}' ;;
  agent\ list) cat "$FIXTURE/agents.json" ;;
  tab\ focus) printf 'tab %s\n' "$3" >> "$FIXTURE/actions" ;;
  agent\ focus)
    printf 'pane %s\n' "$3" >> "$FIXTURE/actions"
    count=$(cat "$FIXTURE/pane-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/pane-count"
    if [ "$count" -eq 1 ]; then printf '%s\n' '{"result":{"agents":[{"workspace_id":"wA","tab_id":"new-tab","pane_id":"new-pane"}]}}' > "$FIXTURE/agents.json"; exit 1; fi
    ;;
  *) exit 1 ;;
esac
"###,
    );
    executable(
        &bin.join("fzf"),
        r###"#!/bin/sh
count=$(cat "$FIXTURE/fzf-count" 2>/dev/null || printf 0); count=$((count + 1)); printf '%s\n' "$count" > "$FIXTURE/fzf-count"
cat > "$FIXTURE/candidates-$count"
sed -n '2p' "$FIXTURE/candidates-$count"
"###,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-sessionizer"))
        .arg("pick")
        .env("HOME", &home)
        .env("FIXTURE", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        format!("{}\n", home.join("VAULT/project").display()).as_bytes()
    );
    assert_eq!(
        fs::read_to_string(root.join("actions")).unwrap(),
        "tab captured-tab\npane captured-pane\ntab captured-tab\npane captured-pane\n"
    );
}
