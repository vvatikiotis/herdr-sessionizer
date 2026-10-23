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
fn defaults_and_empty_or_omitted_configuration_replace_choices_predictably() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();

    // The executable-boundary fallback is covered by the existing discovery
    // regression; this test focuses on present files replacing it completely.
    fs::write(bin.join("directories.toml"), "\n").unwrap();
    let empty_output = run_pick(&home, &bin, root);
    assert!(empty_output.status.success());
    assert_eq!(fs::read_to_string(root.join("candidates")).unwrap(), "\n");

    fs::write(bin.join("directories.toml"), "include = [\"~/explicit\"]\n").unwrap();
    let omitted_output = run_pick(&home, &bin, root);
    assert!(omitted_output.status.success());
    let omitted = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(omitted.contains(&format!("{}\t", home.join("explicit").display())));
    assert!(!omitted.contains("default-project"));
}

#[test]
fn normalized_paths_preserve_order_deduplicate_and_exclude_roots_before_access() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("directories.toml"),
        r#"include = ["~/explicit/", "~/explicit", "/tmp/does-not-matter"]
exclude = ["~/missing-root"]
[[roots]]
path = "~/missing-root/"
depth = 3
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
    assert_eq!(
        candidates
            .lines()
            .filter(|line| line.starts_with(&format!("{}\t", home.join("explicit").display())))
            .count(),
        1
    );
    assert!(candidates.contains("/tmp/does-not-matter"));
    assert!(!candidates.contains("Directory discovery failed"));
}

#[test]
fn invalid_configuration_forms_are_visible_without_using_defaults() {
    let cases = [
        ("unknown = true\n", "unknown field"),
        (
            "roots = [{ path = \"~/root\", depth = 1, extra = true }]\n",
            "unknown field",
        ),
        (
            "[[roots]]\npath = \"~/root\"\ndepth = 0\n",
            "positive integer",
        ),
        (
            "[[roots]]\npath = \"relative\"\ndepth = 1\n",
            "absolute or home-relative",
        ),
        (
            "[[roots]]\npath = \"~/root/../other\"\ndepth = 1\n",
            "may not contain ..",
        ),
        (
            "[[roots]]\npath = \"~/root\"\ndepth = 1\n[[roots]]\npath = \"~/root/.\"\ndepth = 2\n",
            "duplicate directory root",
        ),
    ];
    for (config, expected) in cases {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let (home, bin) = deployment(&temp);
        fs::create_dir_all(root.join("caller")).unwrap();
        fs::write(bin.join("directories.toml"), config).unwrap();
        executable(
            &bin.join("fzf"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FIXTURE/fzf-args\"\ncat > \"$FIXTURE/candidates\"\nexit 0\n",
        );
        let output = run_pick(&home, &bin, root);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let candidates = fs::read_to_string(root.join("candidates")).unwrap();
        assert_eq!(
            candidates, "\n",
            "invalid config produced candidates: {candidates:?}"
        );
        let args = fs::read_to_string(root.join("fzf-args")).unwrap();
        assert!(args.contains("Directory discovery failed"));
        assert!(args.contains(expected), "{expected:?} not in {args:?}");
    }
}

#[test]
fn configured_symlinks_validate_targets_but_explicit_inclusions_remain_unchecked() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    let target = home.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(home.join("file"), "file").unwrap();
    symlink(&target, home.join("root-link")).unwrap();
    symlink(home.join("file"), home.join("file-link")).unwrap();
    symlink(home.join("missing"), home.join("broken-link")).unwrap();
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("directories.toml"),
        "include = [\"~/broken-link\"]\n[[roots]]\npath = \"~/\"\ndepth = 1\n",
    )
    .unwrap();

    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains(&format!("{}\t", home.join("root-link").display())));
    assert!(candidates.contains(&format!("{}\t", home.join("target").display())));
    assert!(!candidates.contains(&format!("{}\t", home.join("file-link").display())));
    assert!(candidates.contains(&format!("{}\t", home.join("broken-link").display())));
}

#[test]
fn candidate_paths_with_spaces_and_shell_punctuation_remain_data() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    let project = home.join("it's project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("directories.toml"),
        "include = [\"~/it's project\"]\n",
    )
    .unwrap();

    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains(&format!("{}\t", project.display())));
}

#[test]
fn discovery_error_is_error_only_even_with_existing_workspace_and_selection_attempt() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::create_dir_all(home.join("project")).unwrap();
    symlink("loop", home.join("loop")).unwrap();
    fs::write(
        bin.join("directories.toml"),
        "[[roots]]\npath = \"~/loop\"\ndepth = 1\n",
    )
    .unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"existing","label":"project","focused":true}]}}' ;;
  'pane list') printf '%s\n' '{"result":{"panes":[{"workspace_id":"existing","cwd":"/tmp/project"}]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'status server') exit 0 ;;
  'workspace focus') printf '%s\n' "$*" >> "$FIXTURE/focused"; exit 0 ;;
  *) exit 1 ;;
 esac
"#,
    );
    executable(
        &bin.join("fzf"),
        "#!/bin/sh\ncat > \"$FIXTURE/candidates\"\nprintf '%s\\n' '/tmp/project\\tproject\\tworkspace|existing'\n",
    );

    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("candidates")).unwrap(), "\n");
    assert!(
        !root.join("focused").exists(),
        "error view allowed navigation"
    );
}

#[test]
fn shipped_configuration_matches_missing_file_defaults() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();

    let without_file = run_pick(&home, &bin, root);
    assert!(without_file.status.success());
    let defaults = fs::read_to_string(root.join("candidates")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("directories.toml"),
        bin.join("directories.toml"),
    )
    .unwrap();
    let with_file = run_pick(&home, &bin, root);
    assert!(with_file.status.success());
    assert_eq!(
        defaults,
        fs::read_to_string(root.join("candidates")).unwrap()
    );
}

#[test]
fn malformed_configuration_fails_strictly_but_unrelated_tracking_still_works() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("directories.toml"), "unknown = true\n").unwrap();
    executable(
        &bin.join("fzf"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$FIXTURE/fzf-args"
cat > "$FIXTURE/candidates"
exit 0
"#,
    );
    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fzf_args = fs::read_to_string(root.join("fzf-args")).unwrap();
    assert!(fzf_args.contains("--header"));
    assert!(fzf_args.contains("Directory discovery failed"));
    assert!(fzf_args.contains("unknown field"));
    assert!(!fzf_args.contains("~/VAULT"));

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
    let jumped = Command::new(bin.join("sessionizer"))
        .arg("jump-back")
        .env("HOME", &home)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        jumped.status.success(),
        "{}",
        String::from_utf8_lossy(&jumped.stderr)
    );
    let after_exit = Command::new(bin.join("sessionizer"))
        .arg("after-pane-exit")
        .env("HOME", &home)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            r#"{"event":"pane_exited","data":{"pane_id":"gone","workspace_id":"gone-workspace"}}"#,
        )
        .output()
        .unwrap();
    assert!(
        after_exit.status.success(),
        "{}",
        String::from_utf8_lossy(&after_exit.stderr)
    );
}
