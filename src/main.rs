use serde_json::Value;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("track") => track_workspace(),
        Some("after-pane-exit") => after_pane_exit(),
        Some("pick") => pick(),
        Some("jump-back") => jump_back(),
        Some(command) => Err(format!("unknown command: {command}").into()),
        None => Err("usage: herdr-sessionizer <pick|jump-back>".into()),
    };

    if let Err(error) = result {
        eprintln!("herdr-sessionizer: {error}");
        std::process::exit(1);
    }
}

fn pick() -> Result<(), Box<dyn Error>> {
    let result = pick_inner();
    if let Some(pane_id) =
        std::env::var_os("HERDR_SESSIONIZER_PANE_ID").filter(|value| !value.is_empty())
    {
        let _ = close_picker_pane(pane_id.to_string_lossy().as_ref());
    }
    result
}

fn pick_inner() -> Result<(), Box<dyn Error>> {
    ensure_server();
    track_workspace()?;
    let workspaces = herdr_json(&["workspace", "list"])?;
    let panes = herdr_json(&["pane", "list"])?;
    let agents = herdr_json(&["agent", "list"])?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let available = discover_directories(&home)?;
    let workspace_list = json_array(&workspaces, "workspaces")?;
    let pane_list = json_array(&panes, "panes")?;
    let agent_list = json_array(&agents, "agents")?;
    let pool_id = workspace_list
        .iter()
        .find(|workspace| {
            workspace.get("label").and_then(Value::as_str) == Some("__herdr_shell_pool__")
        })
        .and_then(|workspace| workspace.get("workspace_id"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let history = fs::read_to_string(state_dir()?.join("workspace-history"))?;
    let current_id = workspace_list
        .iter()
        .find(|workspace| is_truthy(workspace.get("focused")))
        .and_then(|workspace| workspace.get("workspace_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut ordered_ids = Vec::new();
    let mut alternatives = history
        .lines()
        .filter(|id| !id.is_empty() && *id != current_id && *id != pool_id)
        .collect::<Vec<_>>();
    if let Some(previous) = alternatives.first() {
        ordered_ids.push(*previous);
    }
    if !current_id.is_empty() {
        ordered_ids.push(current_id);
    }
    ordered_ids.extend(alternatives.drain(1.min(alternatives.len())..));
    ordered_ids.extend(
        workspace_list
            .iter()
            .filter_map(|workspace| workspace.get("workspace_id").and_then(Value::as_str)),
    );

    let mut seen_ids = std::collections::HashSet::new();
    let mut open_paths = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    for id in ordered_ids {
        if id == pool_id || !seen_ids.insert(id) {
            continue;
        }
        let Some(workspace) = workspace_list
            .iter()
            .find(|workspace| workspace.get("workspace_id").and_then(Value::as_str) == Some(id))
        else {
            continue;
        };
        if let Some(path) = workspace_directory(workspace, &available, pane_list)
            && seen_paths.insert(path.clone())
        {
            open_paths.push(path);
        }
    }

    let open_labels = workspace_list
        .iter()
        .filter(|workspace| workspace.get("workspace_id").and_then(Value::as_str) != Some(pool_id))
        .filter_map(|workspace| workspace.get("label").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    let unopened = available
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !open_labels.contains(name))
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    let width = open_paths
        .iter()
        .map(|path| path.chars().count())
        .max()
        .unwrap_or(0);
    let mut open_rows = Vec::new();
    for path in &open_paths {
        let mut display = abbreviate_home(path, &home);
        let counts = agent_counts(path, workspace_list, agent_list, pool_id);
        if !counts.is_empty() {
            let padding = width.saturating_sub(display.chars().count());
            display = format!("{}{} ({counts})", display, " ".repeat(padding));
        }
        open_rows.push(format!("{path}\t{display}"));
    }
    let directory_rows = unopened
        .iter()
        .map(|path| format!("{path}\t{path}"))
        .collect::<Vec<_>>();
    let temporary = TemporaryDirectory::new()?;
    let open_file = temporary.path.join("open-workspaces");
    let directory_file = temporary.path.join("directories");
    fs::write(&open_file, lines_with_newline(&open_rows))?;
    fs::write(&directory_file, lines_with_newline(&directory_rows))?;
    let selection = run_fzf(&open_file, &directory_file, &open_rows)?;
    if selection.is_empty() {
        return Ok(());
    }
    let selected_path = selection.split('\t').next().unwrap_or(&selection);
    let directory = resolve_directory(selected_path)?;
    let name = Path::new(&directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&directory);
    let workspace_id = workspace_list.iter().find_map(|workspace| {
        (workspace.get("workspace_id").and_then(Value::as_str) != Some(pool_id)
            && workspace.get("label").and_then(Value::as_str) == Some(name))
        .then(|| workspace.get("workspace_id").and_then(Value::as_str))
        .flatten()
    });

    let shell_pool = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("herdr-shell-pool/herdr-shell-pool-bin"))
        .filter(|path| path.is_file());
    if let Some(workspace_id) = workspace_id {
        herdr_silent_stdout(&["workspace", "focus", workspace_id])?;
    } else if let Some(shell_pool) = shell_pool {
        let status = Command::new(shell_pool)
            .args(["new-workspace", "--cwd", &directory, "--label", name])
            .stdout(Stdio::null())
            .status()?;
        if !status.success() {
            // Creation may already have happened. Never retry it blindly.
            return Err(
                "shell-pool workspace creation failed; no creation retry was attempted".into(),
            );
        }
    } else {
        let result = herdr_json(&[
            "workspace",
            "create",
            "--cwd",
            &directory,
            "--label",
            name,
            "--focus",
        ])?;
        if let Some(tab_id) = result
            .get("result")
            .and_then(|result| result.get("tab"))
            .and_then(|tab| tab.get("tab_id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            herdr_silent_stdout(&["tab", "rename", tab_id, name])?;
        }
    }
    track_workspace()?;
    println!("{directory}");
    Ok(())
}

fn ensure_server() {
    if herdr_server_is_ready() {
        return;
    }
    if let Ok(log) = File::create("/tmp/herdr-server.log")
        && let Ok(stderr) = log.try_clone()
    {
        let _ = Command::new("nohup")
            .args(["herdr", "server"])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn();
    }
    for _ in 0..50 {
        if herdr_server_is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn herdr_server_is_ready() -> bool {
    Command::new("herdr")
        .args(["status", "server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn json_array<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, Box<dyn Error>> {
    value
        .get("result")
        .and_then(|result| result.get(name))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Herdr response has no result.{name} array").into())
}

fn discover_directories(home: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let vault = home.join("VAULT");
    let devel = home.join("devel");
    let mut directories = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in [&vault, &devel] {
        discover_under(root, home, 0, &mut directories, &mut seen)?;
    }
    for name in ["dotfiles", "Notes", "Downloads", "Work", "devel", "Mail"] {
        let path = home.join(name);
        if seen.insert(path.to_string_lossy().into_owned()) {
            directories.push(path);
        }
    }
    Ok(directories)
}

fn discover_under(
    directory: &Path,
    home: &Path,
    depth: usize,
    results: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let is_symlink = metadata.file_type().is_symlink();
        let is_directory = metadata.is_dir();
        let next_depth = depth + 1;
        if (is_directory || is_symlink) && next_depth <= 3 {
            if seen.insert(path.to_string_lossy().into_owned()) {
                results.push(path.clone());
            }
            let development_root = home.join("devel");
            let excluded = ["Archive", ".sync", ".SynologyWorkingDirectory"]
                .iter()
                .any(|name| path == development_root.join(name));
            if excluded {
                results.retain(|candidate| candidate != &path);
                seen.remove(&path.to_string_lossy().into_owned());
            } else if is_directory && !is_symlink && next_depth < 3 {
                discover_under(&path, home, next_depth, results, seen)?;
            }
        }
    }
    Ok(())
}

fn workspace_directory(
    workspace: &Value,
    available: &[PathBuf],
    panes: &[Value],
) -> Option<String> {
    let id = workspace.get("workspace_id")?.as_str()?;
    if let Some(label) = workspace.get("label").and_then(Value::as_str)
        && let Some(path) = available
            .iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(label))
    {
        return Some(path.to_string_lossy().into_owned());
    }
    let pane = panes
        .iter()
        .find(|pane| pane.get("workspace_id").and_then(Value::as_str) == Some(id))?;
    pane.get("cwd")
        .filter(|cwd| !cwd.is_null())
        .or_else(|| pane.get("foreground_cwd"))
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn abbreviate_home(path: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    path.strip_prefix(home.as_ref())
        .map(|suffix| format!("~{suffix}"))
        .unwrap_or_else(|| path.to_owned())
}

fn agent_counts(path: &str, workspaces: &[Value], agents: &[Value], pool_id: &str) -> String {
    let name = path.rsplit('/').next();
    let workspace_id = workspaces
        .iter()
        .find(|workspace| {
            workspace.get("workspace_id").and_then(Value::as_str) != Some(pool_id)
                && workspace.get("label").and_then(Value::as_str) == name
        })
        .and_then(|workspace| workspace.get("workspace_id"))
        .and_then(Value::as_str);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for agent in agents
        .iter()
        .filter(|agent| agent.get("workspace_id").and_then(Value::as_str) == workspace_id)
    {
        let name = agent
            .get("agent")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            })
            .unwrap_or_else(|| "null".to_owned());
        *counts.entry(name).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(agent, count)| {
            let display_name = match agent.as_str() {
                "pi" => "Pi",
                "codex" => "Codex",
                "hermes" => "Hermes",
                "opencode" => "OpenCode",
                "openclaw" => "OpenClaw",
                "claude-code" => "Claude Code",
                other => other,
            };
            format!("{display_name}: {count}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn lines_with_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_fzf(
    open_file: &Path,
    directory_file: &Path,
    open_rows: &[String],
) -> Result<String, Box<dyn Error>> {
    let open = shell_quote(&open_file.to_string_lossy());
    let directories = shell_quote(&directory_file.to_string_lossy());
    let reload = format!(
        r#"cat {open}; printf '\n'; fzf --delimiter='\t' --nth=2 --filter={{q}} < {directories}"#
    );
    let first_directory = open_rows.len() + 2;
    let selection = format!(
        r#"if [[ -n {{q}} ]]; then match=$(fzf --delimiter=$'\t' --nth=2 --filter={{q}} < {open} | head -n 1); if [[ -n "$match" ]]; then position=$(grep -Fxn -m 1 -- "$match" {open} | cut -d: -f1); printf 'pos(%s)\n' "$position"; else echo 'pos({first_directory})'; fi; fi"#
    );
    let mut input = fs::read_to_string(open_file)?;
    input.push('\n');
    input.push_str(&fs::read_to_string(directory_file)?);
    let mut child = Command::new("fzf")
        .args([
            "--cycle",
            "--delimiter=\\t",
            "--with-nth=2",
            "--layout=reverse",
            "--bind=ctrl-space:accept",
        ])
        .arg(format!("--bind=change:reload:{reload}"))
        .arg(format!("--bind=result-final:transform:{selection}"))
        .args([
            "--raw",
            "--info=inline",
            "--border=rounded",
            "--border-label= Select Herdr workspace directory ",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("failed to open fzf stdin")?
        .write_all(input.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!("fzf exited with status {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_owned())
}

fn resolve_directory(path: &str) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("cd \"$1\" && pwd")
        .arg("herdr-sessionizer")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(format!("cannot enter directory {path}").into());
    }
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_owned())
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new() -> Result<Self, Box<dyn Error>> {
        let base = std::env::temp_dir();
        loop {
            let id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("herdr-sessionizer-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn jump_back() -> Result<(), Box<dyn Error>> {
    track_workspace()?;
    let state_dir = state_dir()?;
    let previous_path = state_dir.join("previous-workspace");
    let previous_id = fs::read_to_string(&previous_path)
        .unwrap_or_default()
        .trim_end_matches('\n')
        .to_owned();
    if previous_id.is_empty() {
        return Ok(());
    }

    let workspaces = herdr_json(&["workspace", "list"])?;
    let is_open = json_array(&workspaces, "workspaces")?
        .iter()
        .any(|workspace| {
            workspace.get("workspace_id").and_then(Value::as_str) == Some(&previous_id)
        });
    if !is_open {
        match fs::remove_file(previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }

    herdr_command(&["workspace", "focus", &previous_id])?;
    track_workspace()
}

fn after_pane_exit() -> Result<(), Box<dyn Error>> {
    let event: Value = serde_json::from_str(&std::env::var("HERDR_PLUGIN_EVENT_JSON")?)?;
    if event.get("event").and_then(Value::as_str) != Some("pane_exited") {
        return Err("after-pane-exit requires a pane_exited event".into());
    }
    let pane_id = event["data"]["pane_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("pane exit event has no pane identifier")?;
    let workspace_id = event["data"]["workspace_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("pane exit event has no workspace identifier")?;

    // Herdr 0.9.5 emits pane.exited before removing the pane/workspace.
    // Observe removal rather than relying on a fixed delay before tracking.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let panes = herdr_json(&["pane", "list"])?;
        if !json_array(&panes, "panes")?
            .iter()
            .any(|pane| pane.get("pane_id").and_then(Value::as_str) == Some(pane_id))
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("timed out waiting for exited pane removal".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    track_workspace()?;
    let workspaces = herdr_json(&["workspace", "list"])?;
    let workspaces = json_array(&workspaces, "workspaces")?;
    let workspace_remains = workspaces.iter().any(|workspace| {
        workspace.get("workspace_id").and_then(Value::as_str) == Some(workspace_id)
    });
    let pool_focused = workspaces.iter().any(|workspace| {
        workspace.get("label").and_then(Value::as_str) == Some("__herdr_shell_pool__")
            && is_truthy(workspace.get("focused"))
    });
    if workspace_remains || !pool_focused {
        return Ok(());
    }
    let recent = fs::read_to_string(state_dir()?.join("current-workspace"))?;
    let users = workspaces
        .iter()
        .filter(|workspace| {
            workspace.get("label").and_then(Value::as_str) != Some("__herdr_shell_pool__")
        })
        .filter_map(|workspace| workspace.get("workspace_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    // Prefer visit history; if none survives, use an existing user workspace.
    if let Some(target) = users
        .iter()
        .copied()
        .find(|id| *id == recent.trim())
        .or_else(|| users.first().copied())
    {
        herdr_silent_stdout(&["workspace", "focus", target])?;
        track_workspace()?;
    }
    Ok(())
}

fn track_workspace() -> Result<(), Box<dyn Error>> {
    let workspaces = herdr_json(&["workspace", "list"])?;
    let state_dir = state_dir()?;
    fs::create_dir_all(&state_dir)?;

    let history_path = state_dir.join("workspace-history");
    let current_path = state_dir.join("current-workspace");
    let previous_path = state_dir.join("previous-workspace");
    let old_history = match fs::read_to_string(&history_path) {
        Ok(history) => history,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            [&current_path, &previous_path]
                .iter()
                .filter_map(|path| fs::read_to_string(path).ok())
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(error) => return Err(error.into()),
    };

    let open_workspaces = json_array(&workspaces, "workspaces")?
        .iter()
        .filter(|workspace| {
            workspace.get("label").and_then(Value::as_str) != Some("__herdr_shell_pool__")
        })
        .collect::<Vec<_>>();
    let open_ids = open_workspaces
        .iter()
        .filter_map(|workspace| workspace.get("workspace_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let mut history = Vec::new();
    for workspace in open_workspaces {
        if is_truthy(workspace.get("focused"))
            && let Some(id) = workspace.get("workspace_id").and_then(Value::as_str)
        {
            history.push(id.to_owned());
        }
    }
    history.extend(old_history.lines().map(str::to_owned));
    history.retain(|id| !id.is_empty() && open_ids.contains(&id.as_str()));
    let mut seen = std::collections::HashSet::new();
    history.retain(|id| seen.insert(id.clone()));

    let contents = format!("{}\n", history.join("\n"));
    let temp_path = write_temp_file(&state_dir, &contents)?;
    if let Err(error) = fs::rename(&temp_path, &history_path) {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }

    fs::write(
        &current_path,
        history
            .first()
            .map_or_else(String::new, |id| format!("{id}\n")),
    )?;
    if let Some(id) = history.get(1) {
        fs::write(&previous_path, format!("{id}\n"))?;
    } else {
        fs::write(&previous_path, "")?;
    }
    Ok(())
}

fn is_truthy(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null) | Some(Value::Bool(false)))
}

fn herdr_json(args: &[&str]) -> Result<Value, Box<dyn Error>> {
    let output = Command::new("herdr").args(args).output()?;
    std::io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Err(format!("herdr {} failed", args.join(" ")).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn herdr_command(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("herdr").args(args).status()?;
    if !status.success() {
        return Err(format!("herdr {} failed", args.join(" ")).into());
    }
    Ok(())
}

fn herdr_silent_stdout(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("herdr")
        .args(args)
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(format!("herdr {} failed", args.join(" ")).into());
    }
    Ok(())
}

fn close_picker_pane(pane_id: &str) -> Result<(), Box<dyn Error>> {
    let _ = Command::new("herdr")
        .args(["pane", "close", pane_id])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

fn state_dir() -> Result<PathBuf, Box<dyn Error>> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or("HOME is not set")?;
    Ok(root.join("herdr-sessionizer"))
}

fn write_temp_file(directory: &Path, contents: &str) -> Result<PathBuf, Box<dyn Error>> {
    loop {
        let id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("workspace-history.{}.{}", std::process::id(), id));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(contents.as_bytes()) {
                    let _ = fs::remove_file(&path);
                    return Err(error.into());
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
