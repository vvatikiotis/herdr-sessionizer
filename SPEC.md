# Herdr Sessionizer Specification

## Problem Statement

A Herdr user needs one picker to find a project directory, return to an existing workspace, create a workspace, or jump directly to a nested agent. Open workspaces must remain easy to reach while searching for unopened projects. Navigation history must survive command invocations and omit closed or internal pool workspaces.

Project identity is label/basename based. A pane changing directory must not redefine its workspace. The picker must make agent status, navigation failures, omitted workspaces, and directory-configuration failures visible instead of silently choosing another target.

This document describes the independent Herdr Sessionizer plugin: its picker, nested-agent navigation, directory discovery, focus history, jump-back command, and lifecycle hooks. The surrounding Herdr configuration and popup caller remain outside the plugin boundary.

## Solution

Provide a terminal picker with open workspaces first, nested agent rows under their parent workspaces, and unopened directory candidates below a separator. Preserve workspace recency ordering. Selecting a workspace focuses it; selecting an agent focuses its captured tab and then pane; selecting an unopened directory creates and focuses a workspace.

Display every reported agent in the snapshot, including `working`, `done`, `idle`, `blocked`, `unknown`, and unexpected statuses. Render `working` yellow, `done` green, `blocked` red, `idle` with the terminal default color, and `unknown` or unexpected statuses grey. Keep status text readable in addition to color.

Search retains workspace rows. A matching workspace shows all of its agents; a nonmatching workspace shows only agents whose names or types match. The first matching row in display order is selected. Agent-selection failures remain visible and retryable in the open picker.

Read directory candidates from an executable-relative `directories.toml` when present. Ship the current choices, preserve built-in fallback choices when the file is absent, validate the schema and paths strictly, and keep configuration or discovery errors in an error-only picker view until dismissal. The directory configuration affects `pick` only; tracking, jump-back, and pane-exit recovery remain independent.

Maintain persistent, unique workspace visit history through plugin startup, workspace-focus, and pane-exit hooks. Exclude the reserved pool workspace. When shell exit closes a workspace and Herdr falls back to the pool, restore a surviving user workspace. A separate jump-back command toggles between the two most recently visited workspaces that remain open.

## User Stories

1. As a terminal user, I want open and unopened projects in one picker, so that I can switch tasks without assembling commands.
2. As a terminal user, I want open workspaces ordered by recent visits, so that the workspace I just left is easy to reach.
3. As a terminal user, I want open workspaces retained while searching, so that a directory query does not hide existing tasks.
4. As a terminal user, I want each reported agent nested under its workspace, so that I can identify its context.
5. As a terminal user, I want every reported agent shown regardless of status, so that idle, blocked, done, and unknown agents remain reachable.
6. As a terminal user, I want agent names and types displayed, so that I can recognize sessions.
7. As a terminal user, I want duplicate agent labels distinguished by pane ID, so that I can choose the intended session.
8. As a terminal user, I want missing names to use a readable fallback label, so that incomplete metadata does not hide an agent.
9. As a terminal user, I want exact status text and colors, so that the picker does not invent state meanings.
10. As a terminal user, I want `working` yellow, `done` green, `blocked` red, `idle` terminal-default, and unknown or unexpected statuses grey, so that status remains understandable and uncertainty is visible.
11. As a terminal user, I want a stable agent snapshot while choosing, so that retrying does not silently retarget a changed agent record.
12. As a terminal user, I want searches to match workspace and agent name/type fields, so that I can navigate by project or agent.
13. As a terminal user, I want a matching workspace to reveal all of its agents, so that a project search shows its complete agent context.
14. As a terminal user, I want only matching agents under a nonmatching workspace, so that agent searches reduce clutter.
15. As a terminal user, I want the first matching row in display order selected, so that acceptance is predictable across workspaces and agents.
16. As a terminal user, I want agent selection to activate the correct workspace and focus its captured tab, then pane, so that I arrive at the selected agent.
17. As a terminal user, I want missing tab or pane IDs rejected before focus changes, so that incomplete metadata cannot cause partial navigation.
18. As a terminal user, I want stale or failed agent targets reported inside the picker, so that I can retry or dismiss without losing the error.
19. As a terminal user, I want a successful agent selection to print its parent workspace directory and refresh history, so that existing callers retain the directory-output contract.
20. As a terminal user, I want workspaces without usable directory paths named in a persistent warning, so that omitted entries are explainable.
21. As a terminal user, I want project directories discovered under configured roots at configured depths, so that I do not maintain every project manually.
22. As a terminal user, I want explicit included directories, exact exclusions, and duplicate removal, so that candidate discovery is predictable.
23. As a terminal user, I want discovered directory symlinks validated as directory targets without traversing them, so that linked projects are available without cycles.
24. As a terminal user, I want configuration and discovery errors visible until dismissal, so that a temporary popup does not hide the cause.
25. As a terminal user, I want invalid directory configuration not to break tracking, jump-back, or pane-exit recovery, so that unrelated navigation continues working.
26. As a terminal user, I want selecting an existing workspace directory to focus that workspace, so that I do not create a duplicate task.
27. As a terminal user, I want selecting an unopened directory to create and focus a workspace there, so that I can begin work immediately.
28. As a terminal user, I want shell-pool delegation when the optional sibling tool exists, and ordinary Herdr creation when it does not.
29. As a terminal user, I want Ctrl+d in the final shell of a workspace to return from the pool to a surviving user workspace.
30. As a terminal user, I want repeated jump-back invocations to toggle between the two most recent open user workspaces.
31. As a popup user, I want Escape, Ctrl+C, or Enter to dismiss a configuration/discovery error, so that I can acknowledge it without navigating.
32. As a calling program, I want error dismissal to return failure with the error on stderr and stdout empty, so that acknowledgement does not conceal a failed operation.
33. As a user, I want both the original and display errors reported if the error view cannot open, so that a UI failure does not hide the cause.
34. As a maintainer, I want tests for the approved search, invalid-configuration, missing-metadata, and filesystem-error cases, so that passing tests substantiate those requirements.

## Implementation Decisions

### Plugin boundary and runtime

- The user-facing Rust binary accepts `pick` and `jump-back`. Internal `track` and `after-pane-exit` modes support lifecycle hooks.
- The manifest identifies `user.herdr-sessionizer`, requires Herdr 0.9.3 or newer, and declares Linux and macOS support. It registers startup and `workspace.focused` tracking plus `pane.exited` recovery. It does not register a picker key binding.
- Herdr supplies workspace, pane, agent, and server operations. fzf supplies the picker. The plugin owns picker behavior, navigation, history, and temporary-file cleanup.
- Popup size and key bindings are caller-owned. The surrounding caller may set popup height to 95% while leaving width unchanged in its `../config.toml`; that is an integration choice, not a plugin requirement. The plugin must retain its boundary and must not edit that configuration.
- The sibling `herdr-shell-pool/herdr-shell-pool-bin` beside the plugin directory is optional. The reserved label `__herdr_shell_pool__` is excluded even when the executable is absent.

### Workspace, agent, and server contracts

- The picker ensures the Herdr server is ready, then refreshes history and takes workspace, pane, and agent snapshots. Tracking and jump-back do not start the server.
- Workspace records use `workspace_id`, `label`, and `focused`; pane records use `workspace_id`, `cwd`, and `foreground_cwd`; agent records use workspace, tab, pane, display/name/type, and `agent_status` fields as available.
- Agent rows are associated by workspace ID, preserve Herdr's returned order, and retain captured tab and pane IDs. Selection focuses the captured tab first and pane second. It does not search by name, fall back to another target, create a workspace, or roll back a successful tab focus.
- Missing IDs are validation errors before any focus operation. Stale targets and focus failures are visible in the still-open picker and can be retried with the same snapshot or dismissed. The picker pane and temporary files still clean up on exit.
- A successful workspace or agent selection refreshes history before printing the resolved absolute directory. Agent status styling and labels never contaminate that output.

### Directory configuration and discovery

- The picker reads `directories.toml` beside the running executable on every invocation, independent of the caller's working directory. There is no alternate path or environment override.
- The schema has optional top-level `roots`, `include`, and `exclude` lists. Each root has exactly `path` and positive integer `depth`. Unknown keys, invalid types, zero depth, relative paths, and paths containing `..` are errors.
- Paths may be absolute or use `~/`. Normalize redundant separators, dot components, and trailing separators without resolving symlinks. Reject duplicate normalized roots, including duplicates with different depths.
- A present file replaces built-in choices completely. Omitted lists and an empty file mean no discovered or explicitly included candidates. Only a missing file activates built-in fallback choices.
- The confirmed initial configuration and built-in fallback use depth 3 for both roots. The currently checked-in editable file has since set `~/devel` to depth 4; that local configuration value does not change the confirmed default contract.
- The shipped `directories.toml` currently includes `~/dotfiles`, `~/Notes`, `~/Downloads`, `~/Work`, `~/devel`, and `~/Mail`; excludes `~/devel/Archive`, `~/devel/.sync`, and `~/devel/.SynologyWorkingDirectory`; discovers `~/VAULT` at depth 3 and `~/devel` at depth 4. Roots are processed in file order, followed by includes in list order. The first normalized candidate occurrence wins. Depth one means immediate children, not the root itself.
- Exclusions are exact normalized paths. An excluded root is skipped before access; an excluded discovered directory prunes its subtree. Explicit includes are processed independently and may re-add excluded or unavailable paths. Explicit includes are not existence-checked during loading and retain selection-time access behavior.
- Discovered symlinks are offered only when their targets are existing directories and are never traversed. Broken links and links to files are omitted. Other target-check errors are discovery errors. This applies to configured roots and built-in fallback discovery, but not to explicit includes.
- Missing or unreadable roots and other discovery failures stop normal candidate construction. The picker shows an error-only view until dismissal. It must not fall back to defaults, navigate, create a workspace, or affect `track`, `jump-back`, or `after-pane-exit`.

### Configuration/discovery error dismissal

- Configuration and discovery failures show an error-only view with no selectable workspace, agent, or directory actions. Keep it visible until Escape, Ctrl+C, or Enter. Enter acknowledges and dismisses the message; it never accepts a navigation target.
- After dismissal, return a nonzero status, report the original error on stderr, and leave stdout empty. Acknowledgement does not make the failed operation successful. No exact numeric failure code or byte-for-byte message format is required.
- Clean up temporary resources and attempt to close only the explicitly identified temporary picker pane under the existing ownership rules. Do not navigate or create anything as an error-handling action. Closing a caller-owned popup does not promise restoration of its previous focus.
- If fzf cannot open the error view, report both the original failure and display failure on stderr, clean up, and return failure with empty stdout. Do not fall back or continue normal selection. Persistent visual display cannot be guaranteed when the display itself fails.
- These rules apply only to configuration/discovery failures. Agent-selection errors retain their separate retryable flow, allowing another selection against the captured snapshot.

### Picker ordering, display, and search

- Open workspaces appear in previous-first recency order, then the current workspace, remaining visited workspaces, and unvisited open workspaces in Herdr order. The pool is excluded. A separator remains between open and unopened groups, including when the open group is empty.
- Keep workspaces with no agents visible and remove the old workspace-level agent counts. Show display name and agent type when both exist, otherwise whichever exists; use `Agent` with pane ID when neither exists. Disambiguate duplicate name/type labels with pane IDs independently of their statuses. Missing status is `unknown`; unexpected status text remains unchanged. Status/location records are snapshots, with no live refresh during search or retries.
- Omit a workspace and its agents only when no usable directory path can be identified, not because of a new loading-time existence/access check. Preserve label-to-discovered-basename resolution before the first-pane cwd fallback and selection-time directory access checks.
- A workspace row is followed by its nested agent rows. Workspace labels use `~` for the home prefix; unopened directory labels retain full paths. Omitted workspaces are named in a non-selectable warning such as `Hidden workspaces (directory unavailable): project-a, project-b`.
- Empty query shows all agents. On a nonempty query, workspace rows remain available. A matching workspace shows every child agent; otherwise only matching child agents are shown. Unopened directories are filtered independently.
- The first matching row in displayed order is selected, including an earlier matching agent before a later matching workspace. Compute both match and first-directory fallback positions from the filtered displayed list, never the original row count. A no-directory-match filter result is normal, and transporting multiple matching rows must not lose workspace rows on macOS awk.
- If no open workspace or agent row matches, selection falls back to the first unopened-directory position; this does not promise that a directory match exists.
- The picker uses reverse layout, cycling, inline information, a rounded border, ANSI styling, raw mode, and Ctrl-Space acceptance. Successful directory output is the resolved absolute path only. Cancellation and empty successful results do not create or focus a workspace.

### Persistent history and focus recovery

- State is stored under `XDG_STATE_HOME`, or the user's default local state directory when it is empty, in a `herdr-sessionizer` namespace. `workspace-history` is authoritative, newest first; current and previous files are projections.
- When full history is absent, import legacy current and previous state. Retain only open user workspaces, deduplicate IDs, remove closed IDs, and never record `__herdr_shell_pool__`. Replace full history through a temporary file and rename; concurrent invocations are not serialized.
- Pane-exit recovery reads `HERDR_PLUGIN_EVENT_JSON`, waits for the exiting pane to disappear, refreshes history, and redirects focus only when the exiting workspace disappeared and the pool is focused. Prefer the most recent surviving user workspace, otherwise the first existing user workspace. Do not override another user focus or create a workspace.
- Jump-back refreshes history, validates the previous workspace is still open, focuses it, and refreshes history again. It is a successful no-op with no previous target. It does not retry another target when validation fails, and a focus failure does not record a successful visit.
- Existing workspaces are focused by ID. New workspaces delegate to shell pool when available, otherwise use Herdr creation with selected `--cwd`, basename `--label`, and `--focus`, renaming the initial tab when a tab ID is returned. Pool failure is propagated without a blind retry.

## Testing Decisions

- Test the compiled executable through its process boundary with temporary home/state/deployment directories and stub Herdr and fzf programs. Assert observable rows, status styling inputs, focus calls, creation calls, stdout, exit status, cleanup, visible errors, and persistent history, not helper names.
- Use stateful stubs for history, closure, stale IDs, focus failures, configuration replacement, and discovery errors. Use real fzf and isolated Herdr only for terminal behavior that stubs cannot establish.
- The latest audited Rust suite contains 50 test methods: 27 in `tests/commands.rs`, 16 in `tests/directory_config.rs`, 1 in `tests/manifest.rs`, 3 in `tests/shell_pool.rs`, and 3 in `tests/workspace_exit.rs`. Both `cargo test --all-targets` and `cargo test --release --all-targets` passed all 50. `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release` also passed. These commands do not run the separate Python live regressions.
- The passing Rust tests substantiate nested-agent association, status styling inputs and navigation, missing IDs, retryable agent errors, search candidate filtering, workspace retention in candidate streams, directory output, history, shell-pool behavior, common configuration validation/replacement, path normalization, depth/order, exclusions, common symlink cases, and stub-based error/cleanup behavior. They do not establish every requirement below or actual fzf selection/rendering merely by passing.
- The freshly rerun isolated `tests/live_picker.py` regression verified real fzf search/ANSI rendering, nested-agent selection, cross-workspace tab/pane focus, directory output, filtered-position selection, and retryable agent-focus errors without touching the live session. Its bounded state polling waited for settled fzf state before accepting.
- The freshly rerun `tests/live_workspace_exit.py` verified isolated Ctrl+d recovery with the real manifest and Herdr server. It does not prove the full picker-to-warm-workspace flow.
- The freshly rerun `tests/live_config_error.py` passed configuration and discovery error visibility, Escape/Ctrl+C/Enter dismissal, nonzero status, separate stdout/stderr checks, owned-pane cleanup, and failing-fzf error propagation. All three Python live scripts passed on the macOS host with bounded waits. These results establish tested scenarios on this platform, not universal terminal/platform coverage.

### Acceptance coverage from issue #11

- Keep the existing compiled-executable boundary with temporary home/state/deployment directories. Use stub Herdr calls for observable actions and real fzf in isolated PTYs for terminal behavior. No new internal parser test interface is required.
- Search priority: add a real-fzf case where an earlier agent and a later workspace both match. After filtering settles, assert that the earlier agent is highlighted/accepted. A predetermined stub selection is not sufficient evidence.
- Invalid configuration: cover scalar/list mismatches, non-string paths, non-integer depths, and missing required root path/depth fields. Assert clear errors, empty stdout, no navigation/creation, and failure after dismissal.
- Missing metadata: assert the visible `Agent` fallback with pane ID when available and the persistent warning naming omitted workspaces. Verify that warning text is not selectable.
- Filesystem failures: cover unreadable roots and non-NotFound symlink-target errors with deterministic fixtures. If permissions cannot be tested reliably on the host, explicitly report that limitation; another filesystem error is not proof of permission-specific behavior.
- Error dismissal: for each of Escape, Ctrl+C, and Enter, verify that the real error view is visible and the process remains active before the key. Then assert normal dismissal, nonzero status, empty stdout, stderr error, temporary cleanup, and the owned pane-close request. Cover configuration and discovery entry points across these cases.
- Establish a controlling terminal correctly and use bounded waits for observable states. A timeout or force-killed process is a verification failure, not a successful skip of dismissal/cleanup assertions.
- Test unavailable/failing fzf with controlled fixtures; assert both original and display errors, nonzero status, empty stdout, cleanup, and no navigation.
- These remain acceptance requirements even though issue #11 is closed. The current audit covers the approved #13-16 slices, but the map still records partial evidence for several normative requirements. The unreadable-root fixture was tested; permission-denied symlink-target behavior is explicitly unsupported on this macOS host. A symlink-loop regression establishes a non-NotFound target error, not permission-denied behavior. Limit further changes to missing assertions and implementation fixes they prove necessary; do not add unrelated features, refactoring, live-topology changes, or deployment.

## Out of Scope

- The wider Herdr configuration, caller-owned popup size or key choices, themes, terminal settings, and deployment-specific command locations. Popup height 95% with unchanged width is an integration note, not a plugin requirement.
- Shell-pool provisioning, warm-pane movement, destination setup, and replenishment internals.
- Live agent status updates, status-based filtering or ordering, agent control, prompt sending, or status inference.
- Loading-time existence checks for explicit includes, glob exclusions, environment expansion, relative paths, alternate configuration paths, or recursive symlink traversal.
- Unique canonical-path workspace identity, custom workspace naming, sorting discovery results, and changes to basename collision behavior.
- Descriptive fallback notifications and outcome reconciliation after uncertain workspace creation.
- New concurrency locking, transactional multi-file state updates, or guarantees for overlapping event handlers.
- Automatic plugin installation, key-binding registration, package publication, session restoration, closed-workspace resurrection, remote discovery, or project creation.
- Product implementation or expanded tests as part of maintaining this document.

## Further Notes

- GitHub issues #1 through #11 are closed. Directory-configuration completion and the coverage/dismissal follow-up were verified and merged into `main`. Issue closure and merging do not establish deployment or exhaustive test coverage; use actual assertions and recorded runs as evidence.
- Issue #11 supplies the latest confirmed coverage and configuration/discovery dismissal requirements. Its dismissal contract takes precedence over earlier unspecified key/output/status behavior; it does not change retryable agent-selection errors.
- This document distinguishes required behavior from observed implementation and test evidence. Error-dismissal verification is no longer a known failure. Passing the current suite does not establish every requirement: the latest full mapping audit records the remaining partial IDs and the explicit symlink-target permission limitation in `docs/spec-test-coverage-map.md`. Updating this document does not authorize implementation.
- Workspace and directory matching use the first matching label/path in returned or discovered order. Same-basename collisions are not resolved by canonical path.
- The picker may briefly show the pool during asynchronous pane-exit recovery. The reserved label, not the shell-pool executable, controls pool exclusion.
- The plugin remains an independent component. Caller-owned popup integration may use `../config.toml`, but the plugin does not require or modify that integration.
