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
