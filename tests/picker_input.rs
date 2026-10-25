use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn picker_initial_bytes_preserve_empty_groups_and_agent_retry_snapshot() {
    for (has_workspace, has_directory) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let home = root.join("home");
        let project = home.join("project");
        let directory = home.join("unopened");
        let bin = root.join("bin");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&directory).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let binary = bin.join("herdr-sessionizer");
        fs::copy(env!("CARGO_BIN_EXE_herdr-sessionizer"), &binary).unwrap();
        // Configuration belongs only to this copied executable, never the installed binary.
        fs::write(
            bin.join("config.toml"),
            if has_directory {
                format!(
                    "roots = []\ninclude = [{}]\n",
                    serde_json::to_string(&directory).unwrap()
                )
            } else {
                "roots = []\n".to_owned()
            },
        )
        .unwrap();
        fs::write(
            root.join("workspaces.json"),
            if has_workspace {
                r#"{"result":{"workspaces":[{"workspace_id":"wA","label":"project","focused":true}]}}"#
            } else {
                r#"{"result":{"workspaces":[]}}"#
            },
        )
        .unwrap();
        fs::write(
            root.join("panes.json"),
            serde_json::json!({"result":{"panes":[{"workspace_id":"wA","cwd":project}]}})
                .to_string(),
        )
        .unwrap();
        fs::write(root.join("agents.json"), r#"{"result":{"agents":[{"workspace_id":"wA","tab_id":"t1","pane_id":"p1","display_name":"review","agent":"codex","agent_status":"idle"}]}}"#).unwrap();
        executable(
            &bin.join("herdr"),
            r#"#!/bin/sh
case "$1 $2" in
  'status server') exit 0 ;;
  'workspace list') cat "$FIXTURE/workspaces.json" ;;
  'pane list') cat "$FIXTURE/panes.json" ;;
  'agent list') cat "$FIXTURE/agents.json" ;;
  'tab focus') exit 1 ;;
  *) exit 91 ;;
esac
"#,
        );
        executable(
            &bin.join("fzf"),
            r#"#!/bin/bash
for arg in "$@"; do
  case "$arg" in --filter=*) cat; exit 0 ;; esac
done
snapshot=initial
if [ -f "$FIXTURE/initial" ]; then snapshot=retry; fi
printf '%s\n' "$$" > "$FIXTURE/$snapshot-pid"
printf '%s\n' "$@" > "$FIXTURE/$snapshot-args"
for arg in "$@"; do
  case "$arg" in
    --bind=change:reload:*|--bind=result-final:transform:*)
      printf '%s\n' "$arg" >> "$FIXTURE/$snapshot-bindings" ;;
  esac
done
for file in "$TMPDIR"/herdr-sessionizer-*/open-workspaces; do
  cp "$file" "$FIXTURE/$snapshot-open"
  cp "${file%/open-workspaces}/directories" "$FIXTURE/$snapshot-directories"
done
if [ -f "$FIXTURE/initial" ]; then
  cat > "$FIXTURE/retry"
  helper=$(< "$FIXTURE/helper-path")
  /usr/bin/tail -n 1 "$helper" > "$FIXTURE/retry-helper-marker"
  exit 130
fi
cat > "$FIXTURE/initial"
# Capture the callback files while they still exist, through the supplied bindings.
for arg in "$@"; do
  case "$arg" in
    --bind=change:reload:*)
      command=${arg#--bind=change:reload:}
      command=${command//\{q\}/\'\'}
      command=${command/#\/bin\/bash/capture_helper}
      /bin/bash -c '
        capture_helper() {
          printf "%s\n" "$1" > "$FIXTURE/helper-path"
          /bin/bash "$@"
        }
        eval "$1"
      ' fixture "$command" > "$FIXTURE/reloaded"
      helper=$(< "$FIXTURE/helper-path")
      printf '\n# retry marker\n' >> "$helper"
      ;;
  esac
done
if [ "$HAS_WORKSPACE" = 1 ]; then
  # A changed live agent snapshot must not change the retried initial input.
  printf '%s\n' '{"result":{"agents":[]}}' > "$FIXTURE/agents.json"
  sed -n '2p' "$FIXTURE/initial"
else
  exit 130
fi
"#,
        );
        let output = Command::new(&binary)
            .arg("pick")
            .env("HOME", &home)
            .env("FIXTURE", root)
            .env("TMPDIR", root)
            .env("HAS_WORKSPACE", if has_workspace { "1" } else { "0" })
            .env("XDG_STATE_HOME", root.join("state"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let open = if has_workspace {
            format!(
                "{0}\t~/project\tworkspace|wA\t~/project\n{0}\t  ↳ review (codex) \x1b[39m○ [idle]\x1b[0m\tagent|wA|t1|p1\treview codex\n",
                project.display()
            )
        } else {
            String::new()
        };
        let directories = if has_directory {
            format!("{0}\t{0}\tdirectory|{0}\n", directory.display())
        } else {
            String::new()
        };
        let expected = format!("{open}\n{directories}").into_bytes();
        assert_eq!(
            fs::read(root.join("initial-open")).unwrap(),
            open.as_bytes()
        );
        assert_eq!(
            fs::read(root.join("initial-directories")).unwrap(),
            directories.as_bytes()
        );
        assert_eq!(fs::read(root.join("initial")).unwrap(), expected);
        assert_eq!(fs::read(root.join("reloaded")).unwrap(), expected);
        if has_workspace {
            assert_eq!(fs::read(root.join("retry-open")).unwrap(), open.as_bytes());
            assert_eq!(
                fs::read(root.join("retry-directories")).unwrap(),
                directories.as_bytes()
            );
            assert_eq!(fs::read(root.join("retry")).unwrap(), expected);
            let args = fs::read_to_string(root.join("retry-args")).unwrap();
            assert!(args.contains("Agent selection failed:"), "{args}");
            let initial_args = fs::read_to_string(root.join("initial-args")).unwrap();
            assert!(!initial_args.contains("Agent selection failed:"));
            assert_eq!(
                fs::read(root.join("initial-bindings")).unwrap(),
                fs::read(root.join("retry-bindings")).unwrap()
            );
            assert_ne!(
                fs::read(root.join("initial-pid")).unwrap(),
                fs::read(root.join("retry-pid")).unwrap()
            );
            assert_eq!(
                fs::read_to_string(root.join("retry-helper-marker")).unwrap(),
                "# retry marker\n"
            );
        } else {
            assert!(!root.join("retry").exists());
        }
        let helper = fs::read_to_string(root.join("helper-path")).unwrap();
        let temporary = Path::new(helper.trim_end_matches('\n')).parent().unwrap();
        assert!(
            !temporary.exists(),
            "{} was not cleaned up",
            temporary.display()
        );
    }
}
