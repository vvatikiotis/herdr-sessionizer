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

struct OpenWorkspace {
    id: String,
    path: String,
    display: String,
}

struct AgentRow {
    workspace_id: String,
    tab_id: String,
    pane_id: String,
    display: String,
    search: String,
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
    let (available, status_icons) = match discover_directories(&home) {
        Ok(available) => available,
        Err(error) => {
            let error = format!("Directory discovery failed: {error}");
            show_discovery_error(&error)?;
            return Err(error.into());
        }
    };
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
    let mut open_workspaces = Vec::new();
    let mut omitted = Vec::new();
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
        let label = workspace.get("label").and_then(Value::as_str).unwrap_or(id);
        if let Some(path) = workspace_directory(workspace, &available, pane_list) {
            open_workspaces.push(OpenWorkspace {
                id: id.to_owned(),
                display: abbreviate_home(&path, &home),
                path,
            });
        } else {
            omitted.push(label.to_owned());
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

    let width = open_workspaces
        .iter()
        .map(|workspace| workspace.display.chars().count())
        .max()
        .unwrap_or(0);
    let mut open_rows = Vec::new();
    for workspace in &open_workspaces {
        let padding = width.saturating_sub(workspace.display.chars().count());
        open_rows.push(format!(
            "{}\t{}{}\tworkspace|{}\t{}",
            workspace.path,
            workspace.display,
            " ".repeat(padding),
            workspace.id,
            workspace.display
        ));
        let rows = agent_rows(workspace, agent_list, status_icons);
        open_rows.extend(rows.into_iter().map(|row| {
            format!(
                "{}\t  ↳ {}\tagent|{}|{}|{}\t{}",
                workspace.path, row.display, row.workspace_id, row.tab_id, row.pane_id, row.search
            )
        }));
    }
    let directory_rows = unopened
        .iter()
        .map(|path| format!("{path}\t{path}\tdirectory|{path}"))
        .collect::<Vec<_>>();
    let warning = (!omitted.is_empty()).then(|| {
        format!(
            "Hidden workspaces (directory unavailable): {}",
            omitted.join(", ")
        )
    });
    let temporary = TemporaryDirectory::new()?;
    let open_file = temporary.path.join("open-workspaces");
    let directory_file = temporary.path.join("directories");
    fs::write(&open_file, lines_with_newline(&open_rows))?;
    fs::write(&directory_file, lines_with_newline(&directory_rows))?;
    let mut error_header = None;
    let selection = loop {
        let selection = run_fzf(
            &open_file,
            &directory_file,
            picker_header(warning.as_deref(), error_header.as_deref()).as_deref(),
        )?;
        if selection.is_empty() {
            return Ok(());
        }
        let (_selected_path, fields) = selection_fields(&selection);
        if fields.first() == Some(&"agent") {
            match select_agent(&fields, &open_workspaces) {
                Ok(directory) => {
                    println!("{directory}");
                    return Ok(());
                }
                Err(error) => {
                    error_header = Some(format!("Agent selection failed: {error}"));
                    continue;
                }
            }
        }
        break selection;
    };
    let (selected_path, fields) = selection_fields(&selection);
    let selected_path = if fields.first() == Some(&"workspace") {
        let id = fields.get(1).ok_or("workspace row has no identifier")?;
        open_workspaces
            .iter()
            .find(|workspace| workspace.id == *id)
            .map(|workspace| workspace.path.as_str())
            .ok_or("workspace row is unavailable")?
    } else if fields.first() == Some(&"directory") {
        fields.get(1).copied().unwrap_or(selected_path)
    } else {
        selected_path
    };
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryConfig {
    roots: Option<Vec<DirectoryRoot>>,
    #[serde(rename = "include")]
    includes: Option<Vec<String>>,
    #[serde(rename = "exclude")]
    excludes: Option<Vec<String>>,
    status_icons: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryRoot {
    path: String,
    depth: usize,
}

struct LoadedDirectoryConfig {
    roots: Vec<(PathBuf, usize)>,
    includes: Vec<PathBuf>,
    excludes: std::collections::HashSet<PathBuf>,
    excluded_basenames: std::collections::HashSet<String>,
    status_icons: StatusIcons,
}

#[derive(Clone, Copy)]
enum StatusIcons {
    Plain,
    Nerd,
}

fn discover_directories(home: &Path) -> Result<(Vec<PathBuf>, StatusIcons), Box<dyn Error>> {
    let config = load_directory_config(home)?;
    let mut directories = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (root, depth) in &config.roots {
        if config.excludes.contains(root)
            || config.excluded_basenames.contains(
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default(),
            )
        {
            continue;
        }
        discover_under_config(
            root,
            depth,
            0,
            &config.excludes,
            &config.excluded_basenames,
            &mut directories,
            &mut seen,
        )?;
    }
    for path in config.includes {
        if seen.insert(path.clone()) {
            directories.push(path);
        }
    }
    Ok((directories, config.status_icons))
}

fn load_directory_config(home: &Path) -> Result<LoadedDirectoryConfig, Box<dyn Error>> {
    let config_path = std::env::current_exe()?
        .parent()
        .ok_or("running executable has no parent directory")?
        .join("config.toml");
    let config = match fs::read_to_string(&config_path) {
        Ok(contents) => toml::from_str::<DirectoryConfig>(&contents)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DirectoryConfig {
            roots: Some(vec![DirectoryRoot {
                path: "~/".into(),
                depth: 3,
            }]),
            includes: Some(Vec::new()),
            excludes: Some(
                [".git", ".venv", "venv", "node_modules", "__pycache__"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
            status_icons: None,
        },
        Err(error) => return Err(format!("cannot read {}: {error}", config_path.display()).into()),
    };

    if config
        .status_icons
        .as_deref()
        .is_some_and(|icons| !matches!(icons, "plain" | "nerd"))
    {
        return Err("status_icons must be either \"plain\" or \"nerd\"".into());
    }

    let roots = config
        .roots
        .unwrap_or_default()
        .into_iter()
        .map(|root| {
            if root.depth == 0 {
                return Err("directory root depth must be a positive integer".into());
            }
            Ok((normalize_config_path(&root.path, home)?, root.depth))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let mut root_paths = std::collections::HashSet::new();
    for (path, _) in &roots {
        if !root_paths.insert(path.clone()) {
            return Err(format!("duplicate directory root: {}", path.display()).into());
        }
    }
    let includes = config
        .includes
        .unwrap_or_default()
        .into_iter()
        .map(|path| normalize_config_path(&path, home))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let mut excludes = std::collections::HashSet::new();
    let mut excluded_basenames = std::collections::HashSet::new();
    for value in config.excludes.unwrap_or_default() {
        if !value.is_empty()
            && !value.contains('/')
            && !value.contains('\\')
            && value != "."
            && value != ".."
        {
            excluded_basenames.insert(value);
        } else {
            excludes.insert(normalize_config_path(&value, home)?);
        }
    }
    Ok(LoadedDirectoryConfig {
        roots,
        includes,
        excludes,
        excluded_basenames,
        status_icons: match config.status_icons.as_deref() {
            Some("nerd") => StatusIcons::Nerd,
            Some("plain") | None => StatusIcons::Plain,
            Some(_) => unreachable!("status_icons validated above"),
        },
    })
}

fn normalize_config_path(value: &str, home: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let path = if value == "~" {
        home.to_path_buf()
    } else if let Some(suffix) = value.strip_prefix("~/") {
        home.join(suffix)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err(format!("directory path must be absolute or home-relative: {value}").into());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err(format!("directory path may not contain ..: {value}").into());
            }
            std::path::Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn discover_under_config(
    directory: &Path,
    max_depth: &usize,
    depth: usize,
    excludes: &std::collections::HashSet<PathBuf>,
    excluded_basenames: &std::collections::HashSet<String>,
    results: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    if depth >= *max_depth {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let is_symlink = metadata.file_type().is_symlink();
        let is_directory = if is_symlink {
            match fs::metadata(&path) {
                Ok(target) => target.is_dir(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
        } else {
            metadata.is_dir()
        };
        let next_depth = depth + 1;
        if is_symlink && !is_directory {
            continue;
        }
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !(is_directory || is_symlink)
            || next_depth > *max_depth
            || excludes.contains(&path)
            || excluded_basenames.contains(basename)
        {
            continue;
        }
        if seen.insert(path.clone()) {
            results.push(path.clone());
        }
        if is_directory && !is_symlink {
            discover_under_config(
                &path,
                max_depth,
                next_depth,
                excludes,
                excluded_basenames,
                results,
                seen,
            )?;
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

fn agent_rows(workspace: &OpenWorkspace, agents: &[Value], icons: StatusIcons) -> Vec<AgentRow> {
    agents
        .iter()
        .filter(|agent| {
            agent.get("workspace_id").and_then(Value::as_str) == Some(workspace.id.as_str())
        })
        .map(|agent| {
            let display_name = agent
                .get("display_agent")
                .and_then(Value::as_str)
                .or_else(|| agent.get("display_name").and_then(Value::as_str))
                .or_else(|| agent.get("name").and_then(Value::as_str));
            let agent_type = agent
                .get("agent")
                .and_then(Value::as_str)
                .or_else(|| agent.get("type").and_then(Value::as_str));
            let mut label = match (display_name, agent_type) {
                (Some(name), Some(kind)) if name != kind => format!("{name} ({kind})"),
                (Some(name), _) | (None, Some(name)) => name.to_owned(),
                (None, None) => "Agent".to_owned(),
            };
            let pane_id = agent.get("pane_id").and_then(Value::as_str).unwrap_or("");
            let status = agent
                .get("agent_status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let (icon, color) = match status {
                "working" => (
                    match icons {
                        StatusIcons::Plain => "▶",
                        StatusIcons::Nerd => "\u{f04b}",
                    },
                    "33",
                ),
                "done" => (
                    match icons {
                        StatusIcons::Plain => "✓",
                        StatusIcons::Nerd => "\u{f00c}",
                    },
                    "32",
                ),
                "idle" => (
                    match icons {
                        StatusIcons::Plain => "○",
                        StatusIcons::Nerd => "\u{f04c}",
                    },
                    "39",
                ),
                "blocked" => (
                    match icons {
                        StatusIcons::Plain => "!",
                        StatusIcons::Nerd => "\u{f056}",
                    },
                    "31",
                ),
                _ => ("?", "90"),
            };
            let status = format!("\x1b[{color}m{icon} [{status}]\x1b[0m");
            label.push(' ');
            label.push_str(&status);
            let search = match (display_name, agent_type) {
                (Some(name), Some(kind)) => format!("{name} {kind}"),
                (Some(name), None) | (None, Some(name)) => name.to_owned(),
                (None, None) => String::new(),
            };
            AgentRow {
                workspace_id: workspace.id.clone(),
                tab_id: agent
                    .get("tab_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                pane_id: pane_id.to_owned(),
                display: label,
                search,
            }
        })
        .collect()
}

fn selection_fields(selection: &str) -> (&str, Vec<&str>) {
    let row_fields = selection.split('\t').collect::<Vec<_>>();
    let selected_path = row_fields.first().copied().unwrap_or(selection);
    let key = row_fields.get(2).copied().unwrap_or(selected_path);
    (selected_path, key.split('|').collect())
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

fn picker_header(warning: Option<&str>, error: Option<&str>) -> Option<String> {
    match (warning, error) {
        (Some(warning), Some(error)) => Some(format!("{warning}\n{error}")),
        (Some(header), None) | (None, Some(header)) => Some(header.to_owned()),
        (None, None) => None,
    }
}

fn select_agent(
    fields: &[&str],
    open_workspaces: &[OpenWorkspace],
) -> Result<String, Box<dyn Error>> {
    let workspace_id = fields
        .get(1)
        .filter(|id| !id.is_empty())
        .ok_or("agent has no workspace identifier")?;
    let tab_id = fields
        .get(2)
        .filter(|id| !id.is_empty())
        .ok_or("agent has no tab identifier")?;
    let pane_id = fields
        .get(3)
        .filter(|id| !id.is_empty())
        .ok_or("agent has no pane identifier")?;
    let workspace = open_workspaces
        .iter()
        .find(|workspace| workspace.id == *workspace_id)
        .ok_or("agent workspace is unavailable")?;
    herdr_silent_stdout(&["tab", "focus", tab_id])?;
    herdr_silent_stdout(&["agent", "focus", pane_id])?;
    track_workspace()?;
    resolve_directory(&workspace.path)
}

fn show_discovery_error(error: &str) -> Result<(), Box<dyn Error>> {
    let terminal = File::open("/dev/tty").ok();
    let mut child = Command::new("fzf")
        .args([
            "--disabled",
            "--no-multi",
            "--header",
            error,
            "--bind=esc:abort,ctrl-c:abort,enter:abort,ctrl-m:abort,ctrl-j:abort",
        ])
        // Candidate input is empty; keyboard input comes from /dev/tty.
        .stdin(Stdio::null())
        .stdout(terminal.map(Stdio::from).unwrap_or_else(Stdio::null))
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|view_error| format!("{error}; failed to open error view: {view_error}"))?;
    let status = child
        .wait()
        .map_err(|view_error| format!("{error}; failed to wait for error view: {view_error}"))?;
    if status.success() {
        return Err(format!("{error}; error view closed successfully").into());
    }
    if status.code() == Some(130) {
        return Ok(());
    }
    Err(format!("{error}; error view exited with status {status}").into())
}

fn run_fzf(
    open_file: &Path,
    directory_file: &Path,
    header: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let open = shell_quote(&open_file.to_string_lossy());
    let directories = shell_quote(&directory_file.to_string_lossy());
    let filter_file = open_file.with_file_name("filter-open");
    fs::write(
        &filter_file,
        r#"#!/bin/bash
query=$1
open=$2
if [[ -z "$query" ]]; then
    cat "$open"
    exit 0
fi
matches_file=$(mktemp "${TMPDIR:-/tmp}/herdr-sessionizer-matches.XXXXXX") || exit 1
trap 'rm -f -- "$matches_file"' EXIT
fzf --no-sort --delimiter=$'\t' --nth=4 --filter="$query" < "$open" > "$matches_file"
fzf_status=$?
if ((fzf_status != 0 && fzf_status != 1)); then
    exit "$fzf_status"
fi
awk -F'\t' '
    BEGIN {
        while ((getline line < ARGV[1]) > 0) {
            split(line, fields, "\t")
            if (fields[3] != "") matched[fields[3]] = 1
        }
        close(ARGV[1])
        ARGV[1] = ""
    }
    {
        if (index($3, "workspace|") == 1) { print; next }
        split($3, parts, "|")
        if (matched[$3] || matched["workspace|" parts[2]]) print
    }
' "$matches_file" "$open"
"#,
    )?;
    let mut permissions = fs::metadata(&filter_file)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o700);
        fs::set_permissions(&filter_file, permissions)?;
    }
    let filter = shell_quote(&filter_file.to_string_lossy());
    let reload = format!(
        r#"{filter} {{q}} {open}; printf '\n'; fzf --delimiter='\t' --nth=2 --filter={{q}} < {directories} || [[ $? -eq 1 ]]"#
    );
    let selection = format!(
        r#"if [[ -n {{q}} ]]; then match=$(fzf --no-sort --delimiter=$'\t' --nth=4 --filter={{q}} < {open} | head -n 1); filtered=$({filter} {{q}} {open}); if [[ -n "$match" ]]; then identity=$(printf '%s\n' "$match" | awk -F'\t' '{{print $3}}'); position=$(printf '%s\n' "$filtered" | awk -F'\t' -v identity="$identity" '$3 == identity {{print NR; exit}}'); fi; if [[ -n "$position" ]]; then printf 'pos(%s)\n' "$position"; else count=$(printf '%s\n' "$filtered" | awk 'NF {{count++}} END {{print count + 2}}'); printf 'pos(%s)\n' "$count"; fi; fi"#
    );
    let mut input = fs::read_to_string(open_file)?;
    input.push('\n');
    input.push_str(&fs::read_to_string(directory_file)?);
    let mut child = Command::new("fzf")
        .args([
            "--cycle",
            "--ansi",
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
            "--border-label= Select directory to create a workspace ",
        ])
        .args(header.into_iter().flat_map(|header| ["--header", header]))
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
        // fzf uses 130 for an ordinary Escape/Ctrl-C dismissal. Treat it as
        // a normal picker close so temporary panes can clean themselves up.
        if output.status.code() == Some(130) {
            return Ok(String::new());
        }
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
