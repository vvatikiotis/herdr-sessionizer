use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn deployment(temp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp.path();
    let home = root.join("home");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(home.join("root/a/b/c")).unwrap();
    fs::create_dir_all(home.join("other")).unwrap();
    fs::create_dir_all(home.join("explicit")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_herdr-sessionizer"),
        bin.join("sessionizer"),
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[]}}' ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'status server') exit 0 ;;
  *) exit 1 ;;
esac
"#,
    );
    executable(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > \"$FIXTURE/candidates\"\nexit 0\n",
    );
    (home, bin)
}

fn run_pick(home: &Path, bin: &Path, fixture: &Path) -> std::process::Output {
    Command::new(bin.join("sessionizer"))
        .arg("pick")
        .current_dir(fixture.join("caller"))
        .env("HOME", home)
        .env("FIXTURE", fixture)
        .env("XDG_STATE_HOME", fixture.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap()
}

#[test]
fn executable_relative_config_replaces_defaults_and_preserves_order_and_depth() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("directories.toml"),
        r#"include = ["~/explicit", "~/root/a"]
exclude = ["~/root/a/b"]
[[roots]]
path = "~/root"
depth = 2
"#,
    )
    .unwrap();

    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    let root_a = home.join("root/a");
    assert!(
        candidates
            .find(&format!("{}\t", root_a.display()))
            .is_some()
    );
    assert!(
        candidates
            .find(&format!("{}\t", home.join("explicit").display()))
            .is_some()
    );
    assert!(!candidates.contains(&format!("{}\t", home.join("root/a/b").display())));
    assert!(!candidates.contains(&format!("{}\t", home.join("other").display())));
}

#[test]
fn configuration_is_reread_and_lookup_does_not_depend_on_caller_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let config = bin.join("directories.toml");
    fs::write(&config, "include = [\"~/explicit\"]\n").unwrap();
    let first = run_pick(&home, &bin, root);
    assert!(first.status.success());
    assert!(
        fs::read_to_string(root.join("candidates"))
            .unwrap()
            .contains("explicit")
    );

    fs::write(&config, "include = [\"~/other\"]\n").unwrap();
    let second = run_pick(&home, &bin, root);
    assert!(second.status.success());
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("other"));
    assert!(!candidates.contains("explicit"));
}

#[test]
fn malformed_configuration_fails_strictly_but_unrelated_tracking_still_works() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("directories.toml"), "unknown = true\n").unwrap();
    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field"));

    let tracked = Command::new(bin.join("sessionizer"))
        .arg("track")
        .env("HOME", &home)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        tracked.status.success(),
        "{}",
        String::from_utf8_lossy(&tracked.stderr)
    );
}
