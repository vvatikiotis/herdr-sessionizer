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
    fs::create_dir_all(home.join("VAULT")).unwrap();
    fs::create_dir_all(home.join("devel")).unwrap();
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

fn install_agent_fixture(home: &Path, bin: &Path) {
    let agents = serde_json::json!({"result": {"agents": [
        {"workspace_id":"w1", "pane_id":"p0", "agent":"pi", "agent_status":"working"},
        {"workspace_id":"w1", "pane_id":"p1", "agent":"pi", "agent_status":"done"},
        {"workspace_id":"w1", "pane_id":"p2", "agent":"pi", "agent_status":"idle"},
        {"workspace_id":"w1", "pane_id":"p3", "agent":"pi", "agent_status":"blocked"},
        {"workspace_id":"w1", "pane_id":"p4", "agent":"pi", "agent_status":"unknown"},
        {"workspace_id":"w1", "pane_id":"p5", "agent":"pi"},
        {"workspace_id":"w1", "pane_id":"p6", "agent":"pi", "agent_status":"paused"}
    ]}});
    fs::write(bin.join("agents.json"), agents.to_string()).unwrap();
    let panes =
        serde_json::json!({"result":{"panes":[{"workspace_id":"w1","cwd":home.join("other")} ]}});
    fs::write(bin.join("panes.json"), panes.to_string()).unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"w1","label":"other","focused":true}]}}' ;;
  'pane list') /bin/cat "$(dirname "$0")/panes.json" ;;
  'agent list') /bin/cat "$(dirname "$0")/agents.json" ;;
  'status server') exit 0 ;;
  *) exit 1 ;;
esac
"#,
    );
}

#[test]
fn both_icon_modes_color_exact_symbols_and_status_text() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    install_agent_fixture(&home, &bin);
    for (mode, icons) in [
        ("plain", ["▶", "✓", "○", "!", "?", "?", "?"]),
        (
            "nerd",
            [
                "\u{f04b}", "\u{f00c}", "\u{f04c}", "\u{f056}", "?", "?", "?",
            ],
        ),
    ] {
        fs::write(
            bin.join("config.toml"),
            format!("status_icons = \"{mode}\"\n"),
        )
        .unwrap();
        let output = run_pick(&home, &bin, root);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let candidates = fs::read_to_string(root.join("candidates")).unwrap();
        for (index, (status, color)) in [
            ("working", Some("33")),
            ("done", Some("32")),
            ("idle", None),
            ("blocked", Some("31")),
            ("unknown", Some("90")),
            ("unknown", Some("90")),
            ("paused", Some("90")),
        ]
        .iter()
        .enumerate()
        {
            let row = candidates
                .lines()
                .find(|row| row.contains(&format!("agent|w1||p{index}")))
                .unwrap();
            let expected = match color {
                Some(color) => format!("\u{1b}[{color}m{} [{status}]\u{1b}[0m", icons[index]),
                None => format!("{} [{status}]", icons[index]),
            };
            assert!(
                row.contains(&expected),
                "{mode}: {row:?}, expected {expected:?}"
            );
        }
        // A setting alone must not restore the built-in unopened directories.
        assert!(!candidates.contains(&format!("{}\t", home.join("Mail").display())));
    }
}

#[test]
fn icon_defaults_and_invalid_types_are_observable_through_picker() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    install_agent_fixture(&home, &bin);
    for config in [None, Some(""), Some("include = [\"~/explicit\"]\n")] {
        match config {
            Some(contents) => fs::write(bin.join("config.toml"), contents).unwrap(),
            None => {
                let _ = fs::remove_file(bin.join("config.toml"));
            }
        }
        let output = run_pick(&home, &bin, root);
        assert!(output.status.success());
        let candidates = fs::read_to_string(root.join("candidates")).unwrap();
        assert!(candidates.contains("▶ [working]"), "{candidates:?}");
        assert!(!candidates.contains('\u{f04b}'));
    }
    for value in ["true", "12", "[]", "{}"] {
        fs::write(bin.join("config.toml"), format!("status_icons = {value}\n")).unwrap();
        let output = run_pick(&home, &bin, root);
        assert!(!output.status.success(), "accepted {value}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("status_icons"));
    }
}

#[test]
fn executable_relative_config_replaces_defaults_and_preserves_order_and_depth() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("config.toml"),
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
fn legacy_file_is_ignored_and_config_toml_wins_over_conflicting_values() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let baseline = run_pick(&home, &bin, root);
    assert!(baseline.status.success());
    let defaults = fs::read_to_string(root.join("candidates")).unwrap();
    fs::write(bin.join("directories.toml"), "unknown = true\\n").unwrap();

    let legacy_only = run_pick(&home, &bin, root);
    assert!(legacy_only.status.success());
    assert_eq!(
        fs::read_to_string(root.join("candidates")).unwrap(),
        defaults
    );

    fs::write(bin.join("config.toml"), "include = [\"~/other\"]\n").unwrap();
    let configured = run_pick(&home, &bin, root);
    assert!(configured.status.success());
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("other"));
    assert!(!candidates.contains("explicit"));

    fs::write(bin.join("directories.toml"), r#"include = ["~/explicit"]"#).unwrap();
    fs::write(bin.join("config.toml"), "unknown = true\n").unwrap();
    let invalid = run_pick(&home, &bin, root);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Directory discovery failed"));
    assert!(invalid.stdout.is_empty());
}

#[test]
fn configuration_is_reread_and_lookup_does_not_depend_on_caller_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let config = bin.join("config.toml");
    fs::write(
        &config,
        "include = [\"~/explicit\"]\nstatus_icons = \"plain\"\n",
    )
    .unwrap();
    let first = run_pick(&home, &bin, root);
    assert!(first.status.success());
    assert!(
        fs::read_to_string(root.join("candidates"))
            .unwrap()
            .contains("explicit")
    );

    fs::write(
        &config,
        "include = [\"~/other\"]\nstatus_icons = \"nerd\"\n",
    )
    .unwrap();
    let second = run_pick(&home, &bin, root);
    assert!(second.status.success());
    let candidates = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(candidates.contains("other"));
    assert!(!candidates.contains("explicit"));
}

#[test]
fn basename_exclusions_prune_matching_directories_at_any_depth() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    for name in [".git", ".venv", "venv", "node_modules", "__pycache__"] {
        fs::create_dir_all(home.join(format!("root/a/{name}/hidden"))).unwrap();
        fs::create_dir_all(home.join(format!("root/sibling-{name}"))).unwrap();
    }
    fs::write(
        bin.join("config.toml"),
        r#"exclude = [".git", ".venv", "venv", "node_modules", "__pycache__"]
[[roots]]
path = "~/root"
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
    for name in [".git", ".venv", "venv", "node_modules", "__pycache__"] {
        assert!(!candidates.contains(&format!("{name}/")), "{candidates}");
    }
    assert!(candidates.contains(&format!("{}\t", home.join("root/a").display())));
    assert!(candidates.contains(&format!("{}\t", home.join("root/sibling-.git").display())));

    fs::remove_file(bin.join("config.toml")).unwrap();
    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let defaults = fs::read_to_string(root.join("candidates")).unwrap();
    // Defaults search home, not personal project-root conventions.
    assert!(defaults.contains(&format!("{}\t", home.join("other").display())));
    assert!(!defaults.contains(&format!("{}\t", home.join("Mail").display())));
    for name in [".git", ".venv", "venv", "node_modules", "__pycache__"] {
        assert!(
            !defaults.contains(&format!("{}\t", home.join("root/a").join(name).display())),
            "{defaults}"
        );
    }
}

#[test]
fn built_in_defaults_stop_at_depth_three_but_editable_config_can_reach_depth_four() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let deep = home.join("devel/one/two/three/four");
    fs::create_dir_all(&deep).unwrap();

    let fallback = run_pick(&home, &bin, root);
    assert!(fallback.status.success());
    let defaults = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(!defaults.contains(&format!("{}\t", deep.display())));

    fs::write(
        bin.join("config.toml"),
        "[[roots]]\npath = \"~/devel\"\ndepth = 4\n",
    )
    .unwrap();
    let editable = run_pick(&home, &bin, root);
    assert!(
        editable.status.success(),
        "{}",
        String::from_utf8_lossy(&editable.stderr)
    );
    let configured = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(configured.contains(&format!("{}\t", deep.display())));
}

#[test]
fn workspace_directory_uses_first_pane_foreground_cwd_when_cwd_is_missing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let foreground = home.join("foreground");
    let second = home.join("second");
    fs::create_dir_all(&foreground).unwrap();
    fs::create_dir_all(&second).unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"w1","label":"unavailable","focused":true}]}}' ;;
  'pane list') printf '%s\n' '{"result":{"panes":[{"workspace_id":"w1","cwd":null,"foreground_cwd":"'"$HOME"'/foreground"},{"workspace_id":"w1","cwd":"'"$HOME"'/second","foreground_cwd":null}]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'status server') exit 0 ;;
  'workspace focus') printf '%s\n' "$*" >> "$FIXTURE/focused"; exit 0 ;;
  'workspace create') printf '%s\n' '{"result":{}}'; exit 0 ;;
  *) exit 1 ;;
esac
"#,
    );
    executable(
        &bin.join("fzf"),
        r#"#!/bin/sh
printf '%s\t%s\t%s\t%s\n' "$HOME/foreground" unavailable 'workspace|w1' unavailable
"#,
    );

    let output = run_pick(&home, &bin, root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", foreground.display())
    );
}

#[test]
fn defaults_and_empty_or_omitted_configuration_replace_choices_predictably() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();

    // The executable-boundary fallback is covered by the existing discovery
    // regression; this test focuses on present files replacing it completely.
    fs::write(bin.join("config.toml"), "\n").unwrap();
    let empty_output = run_pick(&home, &bin, root);
    assert!(empty_output.status.success());
    assert_eq!(fs::read_to_string(root.join("candidates")).unwrap(), "\n");

    fs::write(bin.join("config.toml"), "include = [\"~/explicit\"]\n").unwrap();
    let omitted_output = run_pick(&home, &bin, root);
    assert!(omitted_output.status.success());
    let omitted = fs::read_to_string(root.join("candidates")).unwrap();
    assert!(omitted.contains(&format!("{}\t", home.join("explicit").display())));
    assert!(!omitted.contains("default-project"));
}

#[test]
fn unavailable_explicit_inclusion_fails_before_focus_or_creation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("config.toml"), "include = [\"~/not-available\"]\n").unwrap();
    executable(
        &bin.join("herdr"),
        r#"#!/bin/sh
case "$1 $2" in
  'workspace list') printf '%s\n' '{"result":{"workspaces":[]}}' ;;
  'pane list') printf '%s\n' '{"result":{"panes":[]}}' ;;
  'agent list') printf '%s\n' '{"result":{"agents":[]}}' ;;
  'status server') exit 0 ;;
  'workspace focus'|'workspace create') printf '%s\n' "$*" >> "$FIXTURE/actions"; exit 1 ;;
  *) exit 1 ;;
esac
"#,
    );
    executable(
        &bin.join("fzf"),
        r#"#!/bin/sh
printf '%s\t%s\t%s\n' "$HOME/not-available" "$HOME/not-available" 'directory|not-available'
"#,
    );

    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot enter directory"));
    assert!(output.stdout.is_empty());
    assert!(!root.join("actions").exists());
}

#[test]
fn normalized_paths_preserve_order_deduplicate_and_exclude_roots_before_access() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("config.toml"),
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
        ("status_icons = \"emoji\"\n", "status_icons must be either"),
        (
            "roots = [{ path = \"~/root\", depth = 1, extra = true }]\n",
            "unknown field",
        ),
        ("include = \"~/root\"\n", "invalid type"),
        ("exclude = {}\n", "invalid type"),
        ("roots = \"~/root\"\n", "invalid type"),
        ("[[roots]]\npath = 1\ndepth = 1\n", "invalid type"),
        (
            "[[roots]]\npath = \"~/root\"\ndepth = \"1\"\n",
            "invalid type",
        ),
        ("[[roots]]\ndepth = 1\n", "missing field `path`"),
        ("[[roots]]\npath = \"~/root\"\n", "missing field `depth`"),
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
        fs::write(bin.join("config.toml"), config).unwrap();
        executable(
            &bin.join("fzf"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FIXTURE/fzf-args\"\n: > \"$FIXTURE/candidates\"\nexit 0\n",
        );
        let output = run_pick(&home, &bin, root);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Directory discovery failed"));
        assert!(output.stdout.is_empty());
        let candidates = fs::read_to_string(root.join("candidates")).unwrap();
        assert_eq!(
            candidates, "",
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
        bin.join("config.toml"),
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
fn permission_denied_fixtures_are_checked_or_reported_as_unsupported() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    let protected = home.join("protected");
    fs::create_dir_all(&protected).unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o000)).unwrap();
    fs::write(
        bin.join("config.toml"),
        "[[roots]]\npath = \"~/protected\"\ndepth = 1\n",
    )
    .unwrap();
    let output = run_pick(&home, &bin, root);
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o755)).unwrap();
    let denied_root = !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("Directory discovery failed");
    let target = home.join("protected-target");
    fs::create_dir_all(&target).unwrap();
    symlink(&target, home.join("protected-link")).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();
    fs::write(
        bin.join("config.toml"),
        "[[roots]]\npath = \"~/\"\ndepth = 1\n",
    )
    .unwrap();
    let target_output = run_pick(&home, &bin, root);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let denied_target = !target_output.status.success()
        && String::from_utf8_lossy(&target_output.stderr).contains("Directory discovery failed");
    if !denied_root {
        eprintln!("permission-denied root fixture is unsupported on this host");
    }
    if !denied_target {
        eprintln!("permission-denied symlink-target fixture is unsupported on this host");
    }
}

#[test]
fn non_not_found_symlink_target_errors_stop_discovery() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    symlink("loop", home.join("root/loop")).unwrap();
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(
        bin.join("config.toml"),
        "[[roots]]\npath = \"~/root\"\ndepth = 2\n",
    )
    .unwrap();
    executable(
        &bin.join("fzf"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FIXTURE/fzf-args\"\nexit 0\n",
    );

    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let args = fs::read_to_string(root.join("fzf-args")).unwrap();
    assert!(args.contains("Directory discovery failed"));
    assert!(args.contains("Too many levels of symbolic links"));
}

#[test]
fn candidate_paths_with_spaces_and_shell_punctuation_remain_data() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    let project = home.join("it's project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("config.toml"), "include = [\"~/it's project\"]\n").unwrap();

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
        bin.join("config.toml"),
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
        "#!/bin/sh\n: > \"$FIXTURE/candidates\"\nprintf '%s\\n' '/tmp/project\\tproject\\tworkspace|existing'\n",
    );

    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Directory discovery failed"));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(root.join("candidates")).unwrap(), "");
    assert!(
        !root.join("focused").exists(),
        "error view allowed navigation"
    );
}

#[test]
fn example_configuration_is_complete_and_accepted() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::create_dir_all(home.join("Projects/client-apps/retired")).unwrap();
    fs::create_dir_all(home.join("Sandbox")).unwrap();
    fs::create_dir_all(home.join("Documents/example-notes")).unwrap();

    let example_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let example = fs::read_to_string(&example_path).unwrap();
    for directive in [
        "status_icons = \"plain\"",
        "include = [",
        "exclude = [",
        "path = \"~/Projects/client-apps\"\ndepth = 3",
        "path = \"~/Sandbox\"\ndepth = 4",
        "~/Projects/client-apps/retired",
    ] {
        assert!(example.contains(directive), "missing {directive}");
    }
    fs::copy(example_path, bin.join("config.toml")).unwrap();
    let with_file = run_pick(&home, &bin, root);
    assert!(with_file.status.success());
}

#[test]
fn error_view_launch_failure_reports_original_and_display_errors() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("config.toml"), "unknown = true\n").unwrap();
    fs::remove_file(bin.join("fzf")).unwrap();

    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Directory discovery failed"));
    assert!(error.contains("failed to open error view"));
}

#[test]
fn malformed_configuration_fails_strictly_but_unrelated_tracking_still_works() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (home, bin) = deployment(&temp);
    fs::create_dir_all(root.join("caller")).unwrap();
    fs::write(bin.join("config.toml"), "unknown = true\n").unwrap();
    executable(
        &bin.join("fzf"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$FIXTURE/fzf-args"
cat > "$FIXTURE/candidates"
exit 0
"#,
    );
    let output = run_pick(&home, &bin, root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Directory discovery failed"));
    assert!(output.stdout.is_empty());
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
