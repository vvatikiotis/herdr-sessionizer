# Herdr Sessionizer Specification

## Problem Statement

A Herdr user needs to find a project directory, return to its existing workspace, or create a workspace without assembling terminal commands. Open workspaces should remain easy to reach while searching for unopened projects, and the user should be able to switch back to the last visited workspace even after changing focus through the sidebar.

Project identity should not change because a pane moves into a subdirectory. Navigation history should survive command invocations and skip workspaces that have closed.

This document describes the existing, independent Herdr Sessionizer plugin: its directory picker, focus-history tracker, jump-back command, and plugin lifecycle hooks. It is not a specification for the surrounding Herdr configuration.

## Solution

Provide a terminal directory picker with open workspaces first and other project directories below a separator. Order open workspaces by visit history, with the previous workspace before the current workspace. Show running-agent counts beside open workspace entries.

Selecting a directory focuses the workspace with the same directory basename, if one exists. Otherwise, use the sibling shell-pool tool when installed to create a workspace with a warm shell, matching workspace/tab names, and the selected directory. If the tool is absent, create and focus a workspace through Herdr and name its initial tab when Herdr returns a tab identifier.

Maintain persistent, unique workspace visit history through plugin startup and workspace-focus events. A separate jump-back command uses that history to toggle between the two most recently visited workspaces that remain open.

The plugin owns these behaviors. Herdr supplies workspace, pane, agent, and server operations; fzf supplies the picker. Host key bindings and popup presentation are caller-owned integrations.

## User Stories

1. As a terminal user, I want one directory picker for open and unopened projects, so that I can switch tasks without choosing separate commands.
2. As a terminal user, I want the picker to start the Herdr server when necessary, so that I can invoke it before the server is running.
3. As a terminal user, I want project directories discovered under my vault and development roots, so that I do not maintain a complete project list manually.
4. As a terminal user, I want common personal directories included explicitly, so that I can open workspaces outside those discovery roots.
5. As a terminal user, I want archive and synchronization directories excluded from development discovery, so that they do not clutter project choices.
6. As a terminal user, I want duplicate directory candidates removed, so that each path appears once within its candidate group.
7. As a terminal user, I want open workspaces shown above unopened directories, so that existing tasks remain immediately accessible.
8. As a terminal user, I want the previous workspace listed first, so that returning to the task I just left takes little effort.
9. As a terminal user, I want the current workspace listed after the previous workspace, so that I can recognize my current context.
10. As a terminal user, I want older visited workspaces ordered by recency, so that frequently revisited tasks remain easy to find.
11. As a terminal user, I want open workspaces without visit history included too, so that the picker does not hide existing tasks.
12. As a terminal user, I want the internal shell-pool workspace excluded from the picker, visit history, and jump-back, so that implementation infrastructure is never treated as a project.
13. As a terminal user, I want a workspace label matched to a discovered directory before using a pane's working directory, so that changing directories inside a pane does not redefine the project.
14. As a terminal user, I want a pane-directory fallback when no discovered directory matches a workspace label, so that workspaces outside the discovery roots can still appear.
15. As a terminal user, I want directories whose basenames already label open workspaces omitted from the unopened group, so that they are not offered as new workspaces.
16. As a terminal user, I want unopened directory choices available when no user workspace exists, so that I can create my first workspace.
17. As a terminal user, I want home-relative labels for open workspace paths, so that long paths are easier to scan.
18. As a terminal user, I want per-agent counts beside open workspaces, so that I can see where agent sessions are running.
19. As a terminal user, I want open workspaces to remain visible while my query filters unopened directories, so that searching does not remove my existing tasks from view.
20. As a terminal user, I want the picker to move to a matching open workspace when possible, so that accepting a search favors an existing task.
21. As a terminal user, I want Ctrl-Space to accept the selected entry, so that I can use that shortcut in addition to the picker's normal acceptance behavior.
22. As a terminal user, I want selecting an existing workspace's directory to focus that workspace, so that I do not create a duplicate task.
23. As a terminal user, I want selecting an unopened directory to create and focus a workspace there, so that I can begin work immediately.
24. As a terminal user, I want new workspaces and their initial tabs named after the selected directory, so that I can recognize them later.
25. As a calling program, I want a successful selection to print its resolved absolute directory, so that I can consume the result without parsing display annotations.
26. As a terminal user, I want leaving the picker without a selection to avoid creating or focusing a workspace, so that I can dismiss it safely.
27. As a popup caller, I want an explicitly identified picker pane closed on exit, so that the temporary picker does not remain open.
28. As a terminal user, I want workspace focus changes tracked outside the picker too, so that sidebar navigation updates my previous-workspace target.
29. As a terminal user, I want repeated visits to promote a workspace without duplicating it in history, so that recency remains meaningful.
30. As a terminal user, I want history retained in my user state directory, so that separate invocations share navigation context.
31. As an existing user, I want legacy current/previous state imported when full history is absent, so that upgrading preserves my recent navigation.
32. As a terminal user, I want closed workspace identifiers removed from history, so that jump-back can choose a remaining visited workspace.
33. As a terminal user, I want repeated jump-back invocations to alternate between the latest two available workspaces, so that I can move between tasks without reopening the picker.
34. As a terminal user, I want jump-back to do nothing when no previous workspace remains, so that zero or one open workspace is handled safely.
35. As a terminal user, I want a failed jump-back focus operation to leave the previous target available, so that a failed switch does not count as a successful visit.
36. As a plugin installer, I want declared startup and focus-event hooks and platform/version requirements, so that Herdr can run the history tracker through its plugin lifecycle.

## Implementation Decisions

### Plugin boundary and runtime

- Keep three command responsibilities: interactive selection, focus-history tracking, and jump-back. The user-facing Rust binary accepts `pick` and `jump-back`; the manifest invokes its `track` mode for lifecycle hooks. The picker and jump-back invoke the same tracker rather than implementing separate history rules.
- Implement the commands in one Rust binary. Herdr and fzf remain external runtime tools; Rust parses Herdr JSON instead of requiring jq. The Cargo build output must be installed beside the plugin manifest as `herdr-sessionizer`.
- The manifest identifies the plugin as `user.herdr-sessionizer`, display name `Herdr Sessionizer History`, version `0.1.0`, minimum Herdr version `0.9.3`, with Linux and macOS declared as supported platforms.
- Register `./herdr-sessionizer track` for plugin startup and `workspace.focused`, and `./herdr-sessionizer after-pane-exit` for `pane.exited`. The manifest does not register a picker key binding or automatically launch the picker.
- The shell-pool executable is an optional integration. Look for `herdr-shell-pool/herdr-shell-pool-bin` beside the sessionizer plugin directory, resolved from the running executable's location. Keep the reserved workspace label `__herdr_shell_pool__` excluded from picker choices. No manifest dependency, key-binding change, or PATH lookup is added.

### Server and Herdr contracts

- The picker checks `herdr status server`. If unavailable, it starts `herdr server` detached, redirects server output to a temporary-system log, and checks readiness up to 50 times with 0.1-second waits. It then proceeds; it does not implement a separate timeout error message.
- The tracker and jump-back do not start the server themselves.
- Consume workspace records from `result.workspaces`, using `workspace_id`, `label`, and `focused`; pane records from `result.panes`, using `workspace_id`, `cwd`, and `foreground_cwd`; and agent records from `result.agents`, using `workspace_id` and `agent`.
- Refresh history before building picker candidates, then take workspace, pane, and agent snapshots. Do not read Herdr's saved session document to infer project identity.
- Focus an existing workspace by identifier without invoking shell pool. For a new workspace when the sibling tool exists, invoke it with `new-workspace --cwd <selected-directory> --label <basename>`, discard its stdout, and require successful exit before history refresh and directory output. The tool owns destination setup, initial-tab naming, focus, and replenishment.
- On tool failure, propagate an error without retrying ordinary workspace creation, because a destination may already exist. This first slice does not deliver a Herdr failure notification.
- When the tool is absent, preserve ordinary creation with the selected directory as `--cwd`, its basename as `--label`, and `--focus`. Rename the initial tab only if the response contains a nonempty `result.tab.tab_id`. A missing tab identifier does not prevent history refresh or directory output. This existing path remains silent; descriptive fallback notifications are not implemented yet.

### Directory discovery and workspace identity

- Discover directory and symbolic-link entries under the user's `VAULT` and `devel` roots, using the existing find expression with directory depths one through three. Prune the development root's `Archive`, `.sync`, and `.SynologyWorkingDirectory` entries.
- Append the user's `dotfiles`, `Notes`, `Downloads`, `Work`, `devel`, and `Mail` directories explicitly. Preserve first occurrence when deduplicating path strings.
- The roots and supplemental directory names are currently fixed conventions, not settings exposed by the plugin. Explicit entries are not checked for existence before presentation.
- For each open user workspace, first use the first discovered path whose basename matches its label. Otherwise use the first pane belonging to that workspace, preferring `cwd` and then `foreground_cwd` when the former is null or absent. Do not search additional panes for a better fallback.
- Omit workspaces without a usable resolved candidate path. Deduplicate resolved open-workspace paths.
- Exclude an available directory from the unopened group when its basename matches any open user-workspace label. This rule must also work when the labels list is empty.
- On selection, enter the directory and resolve it with the shell's `pwd`, then derive its basename. Use the first non-pool workspace with that label as the existing-workspace match.
- Workspace identity is basename/label based, not a canonical-path registry. Different directories with the same basename can therefore map to the same workspace; directory discovery order can affect which matching path is displayed.

### Picker ordering, display, and interaction

- Present open workspaces in this order: most recent visited alternative, current workspace, remaining visited alternatives, then open workspaces absent from history in Herdr's returned order. Remove duplicate workspace identifiers and exclude the pool workspace throughout picker ordering.
- Put a blank separator between open-workspace entries and unopened directory entries, including when the open group is empty.
- Store each candidate as an actual path field followed by a tab-separated display field. Give fzf the display field while retaining the path for selection processing.
- Abbreviate the home prefix to `~` for open-workspace display labels. Unopened-directory labels retain their full paths.
- Count agent records by finding the first non-pool workspace whose label matches the displayed path's basename, then grouping its agents by type. This can differ from the workspace that supplied a pane-fallback path when that basename does not match the workspace label. Display recognized names as Pi, Codex, Hermes, OpenCode, OpenClaw, and Claude Code; retain unrecognized names. Append counts only when present, padding annotated open paths for alignment.
- Use a reverse-layout, cycling picker with inline information, a rounded border, a directory-selection border label, raw mode, and Ctrl-Space acceptance.
- On query changes, reload all open entries unchanged, the separator, and unopened entries filtered by fzf against their display field.
- For a nonempty query after results settle, select the first matching open entry in the original open-workspace order. If none matches, target the first unopened-directory position. This behavior does not assert that a matching unopened entry exists.
- Strip the display field before acting on the selection. Print the selected absolute directory only after the successful focus/create and subsequent history refresh.
- An empty successful picker result exits without workspace selection. A normal fzf cancellation may return nonzero and propagate through strict shell handling; do not promise a zero cancellation status.

### Persistent history

- Store state under `XDG_STATE_HOME` when nonempty, otherwise the user's default local-state directory, within a `herdr-sessionizer` namespace.
- Treat `workspace-history` as the authoritative newline-delimited sequence of workspace identifiers, newest first. Maintain `current-workspace` and `previous-workspace` as its first- and second-entry projections.
- When full history is absent, seed it from the legacy current and previous projections in that order. An existing full-history file takes precedence, even if empty. If a refresh produces no identifiers, write one newline to the full-history file and empty projection files.
- On each refresh, prepend focused workspace identifiers from the current Herdr response, then process stored history. Retain only identifiers still open and preserve each once. Do not add unvisited, unfocused workspaces to history merely because they are open.
- Write full history through a temporary file in the state directory and rename it into place. Update the two projections afterward. This makes replacement of full history atomic, but does not make all three files a transaction or serialize concurrent invocations.
- Exclude workspaces labelled `__herdr_shell_pool__` before collecting focused or open history identifiers. Never record the pool as a visit, even when it is focused. Remove saved pool entries and legacy current/previous pool entries during refresh, then rebuild both projections from the remaining user-workspace history. If only the pool remains, retain no current or previous user workspace.

### Focus after shell exit

- Herdr 0.9.5 emits `pane.exited` before removing a dead pane; it does not emit `workspace.closed` or `workspace.focused` for the resulting automatic workspace removal. Watching workspace-focus events alone cannot correct Ctrl+d fallback.
- The pane-exit command reads the exiting pane and workspace identifiers from `HERDR_PLUGIN_EVENT_JSON`. Poll the pane list until the pane is absent, with a two-second retry window, then refresh history.
- Recheck workspace state. Only if the exiting workspace has disappeared and Herdr currently focuses the reserved pool workspace, focus the most recent surviving user workspace and refresh history again. If history has no valid target, use the first existing non-pool workspace.
- Do not override another user workspace's focus, redirect an explicit pool selection through ordinary tracking, or redirect when only a pane closed and its workspace remains. If no user workspace remains, do not create one automatically.
- The correction is asynchronous; Herdr may briefly focus the pool before the hook redirects it. It uses the pool label only and does not require the shell-pool executable.

### Jump-back and cleanup

- Refresh history before reading the previous projection, so outside focus changes and workspace closures affect the target.
- Exit successfully without focusing anything when no previous identifier remains.
- Re-list workspaces and verify that the chosen identifier is still open. If absent, remove the previous projection and exit without focusing; do not retry another target during that invocation.
- Focus a valid previous workspace, then refresh history again. The old current workspace becomes the next previous target, producing a toggle.
- Propagate focus failure before the final history refresh. The failed target is not recorded as a successful new visit.
- The picker removes its temporary candidate directory on exit. If `HERDR_SESSIONIZER_PANE_ID` is nonempty, it also attempts to close that pane, including on failure or cancellation, ignoring pane-close errors.
- Refresh history after successful focus or creation even when event-driven tracking is also installed. Duplicate visits must remain harmless.

## Testing Decisions

### Boundary and test quality

- Test the compiled Rust executable through its process boundary with temporary home/state directories and stub Herdr and fzf programs on `PATH`. No internal test interface is needed.
- Assert observable candidate lists, focus/create/rename calls, selected-directory output, exit status, cleanup, and persistent history behavior. Avoid tests tied to helper-function names or intermediate variables.
- Use stateful Herdr stubs for multi-step visit, closure, and toggle scenarios. Use an fzf stub to capture candidate input and return controlled selections.
- Stub tests do not establish actual fzf behavior or event delivery. An opt-in real-Herdr regression creates an isolated server, links the actual plugin manifest, exits a test shell with Ctrl+d, and verifies return to the original user workspace. It does not use or modify the user's live session or registry. Real fzf integration remains deferred.

### Existing automated coverage

The Rust integration tests execute the compiled binary through this boundary. The original fourteen test methods cover:

1. Focused-workspace promotion, removal of closed IDs, and unique history ordering.
2. Migration from legacy current/previous state when full history is absent.
3. Previous-first picker ordering and focusing the selected existing workspace.
4. First-workspace creation when only the pool workspace is open, including initial-tab rename.
5. Workspace-label path resolution before pane fallback.
6. Agent-count annotations and empty successful picker selection, including optional pane close.
7. Directory discovery depth, symbolic links, and development-directory pruning.
8. Jump-back toggling between the two most recent open workspaces.
9. Preserving the previous target when focus fails.
10. Handling a previous workspace that closes before jump-back validates it, without retrying.
11. Successful no-op jump-back with one or zero open workspaces.
12. Using the default state directory when `XDG_STATE_HOME` is empty.
13. Giving an existing empty full-history file precedence over legacy projections.
14. Removing picker temporary files and closing its pane after fzf fails.

A further executable-level test installs a fake sibling shell-pool tool and verifies that new-project selection delegates the selected directory and name, suppresses tool stdout, and refreshes history without ordinary workspace creation or tab rename. This checks the integration contract, not a live Herdr move.

Two history regression tests verify pool exclusion: one covers a focused pool, saved pool entries, legacy state import, and a pool-only session; the other confirms jump-back skips a saved pool entry and preserves user-workspace history. A standalone-installation test copies sessionizer into its deployed directory layout without a sibling shell-pool directory. It verifies ordinary new-workspace creation and tab naming, existing-workspace focus, jump-back, selected-directory output, and history cleanup despite a focused, previously recorded pool workspace. Two pane-exit regression methods verify waiting for removal before restoring the most recent user workspace, and preserving focus for another user workspace, a surviving source workspace, or a pool-only session. The current Rust suite has twenty test methods.

The opt-in `tests/live_workspace_exit.py` regression reproduces the native Ctrl+d focus fallback using Herdr 0.9.5 and the real plugin manifest. It was observed failing before the fix and passing afterward in an isolated server. A reserved-label stand-in workspace suffices; no shell-pool installation is needed.

### Additional checks not established by those tests

- Existing-workspace selection, optional initial-tab rename, agent-count annotations, and successful selected-path output for both focus and creation.
- Real fzf query reloads, result selection, separator handling, Ctrl-Space acceptance, no-match behavior, and cancellation status.
- Discovery pruning and depth, symbolic links, missing roots or explicit directories, spaces and shell-sensitive characters, basename collisions, and fallback paths without matching labels.
- Server startup success and readiness exhaustion, malformed or failed Herdr responses, and failures after workspace creation.
- Default state-directory behavior, full-history precedence over legacy projections, and target closure between jump-back's refresh and validation.
- Temporary-directory removal and optional pane-close attempts on successful, empty, canceled, and failed picker runs.
- Startup-hook delivery and lifecycle behavior beyond the tested Herdr 0.9.5/macOS shell-exit scenario. The isolated regression establishes manifest loading and pane-exit/focus hook delivery for that scenario only; Linux remains unverified.

## Out of Scope

- The wider Herdr configuration, its key choices, popup size, themes, terminal settings, or deployment-specific absolute command locations.
- Shell-pool provisioning, warm-pane movement, destination setup, and replenishment internals, which belong to the shell-pool tool. Sessionizer only delegates new workspace creation when that optional tool is installed.
- Tab/split creation policies, last-tab restoration, and editor navigation plugins.
- Descriptive fallback/failure notifications and outcome reconciliation after uncertain workspace creation.
- Herdr Navigator's picker, history, pinning, and independent jump-back implementation.
- Installing or enabling the plugin automatically, registering new picker commands in Herdr, or publishing a package.
- Session snapshot restoration, closed-workspace resurrection, remote project discovery, project creation, or filesystem changes beyond temporary files and navigation state.
- Configurable discovery roots, custom workspace naming, unique path-based workspace identity, or changes to current basename-collision behavior.
- New concurrency locking, transactional multi-file state updates, custom error recovery, or guarantees for overlapping event handlers.
- Treating agent counts as health, completion, or activity-status indicators.
- New implementation work or expanded tests as part of generating this document.

## Further Notes

- This document records observed behavior, not a normative policy for cases the code does not define. Failures of required Herdr operations and fzf propagate as nonzero command results; malformed Herdr JSON fails during Rust JSON parsing. Server-status failures trigger startup and polling, but readiness exhaustion has no dedicated error. Missing or unreadable discovery roots can make the picker fail.
- Workspace and directory matching uses the first matching label/path in the order returned or discovered. Same-basename collisions are not resolved by canonical path.
- A nonempty picker query with no matching open workspace targets the first unopened-entry position, even when no unopened entry matches. Cancellation may return nonzero. These are current implementation behaviors, not promises of a preferred user experience.
- History replacement is atomic only for the full-history file. Tracker invocations are not serialized, so overlapping updates can race; the spec does not define a concurrency guarantee.
- Rust tests use stub Herdr and fzf programs; the opt-in Python shell-exit regression additionally uses a real disposable Herdr server.
- Additional-check items identify behavior not established by automated tests. They do not, by themselves, define missing product requirements or approved feature work.
- The document uses the to-spec section format and is stored locally; it is not published to an issue tracker.
- All nine Bash test methods passed when the original implementation was inspected. Rust process-level tests now cover the replacement implementation. A real Herdr shell-exit regression is available; real fzf integration remains deferred.
- The plugin manifest's display name emphasizes history, but this specification covers the complete sessionizer, including the picker and jump-back entry points.
