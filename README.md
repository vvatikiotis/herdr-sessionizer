# Herdr Sessionizer

Build the plugin binary before enabling or updating the plugin:

```sh
cargo build --release &&
cp target/release/herdr-sessionizer ./herdr-sessionizer.new &&
chmod 755 ./herdr-sessionizer.new &&
mv -f ./herdr-sessionizer.new ./herdr-sessionizer
```

The temporary-file-and-rename sequence avoids overwriting a running executable. Keep `config.toml` beside the binary. For an external deployment, install the binary and a copy of `config.toml` together without overwriting an existing user configuration:

```sh
install -m 755 target/release/herdr-sessionizer "$PLUGIN_DIR/herdr-sessionizer"
if [ ! -e "$PLUGIN_DIR/config.toml" ]; then
  install -m 644 config.toml "$PLUGIN_DIR/config.toml"
fi
```

When upgrading from the former `directories.toml` layout, manually transfer its settings into `config.toml`. The plugin reads only `config.toml`; it never reads or merges `directories.toml`, and it does not automatically migrate settings. The install step above preserves an existing `config.toml`.

Relink after changing the manifest so Herdr reads its current event hooks:

```sh
herdr plugin link . --enabled
```

Herdr runs `./herdr-sessionizer track` at startup and after workspace-focus events, and `after-pane-exit` on pane-exit events. The reserved shell-pool workspace is excluded from visit history and jump-back, even if focused. A history refresh also removes previously saved pool entries, including legacy current/previous state. User-facing commands are:

```sh
./herdr-sessionizer pick
./herdr-sessionizer jump-back
```

For unopened directories, sessionizer optionally calls `herdr-shell-pool/herdr-shell-pool-bin` beside its own plugin directory with `new-workspace --cwd <directory> --label <basename>`. Build and install compatible updated versions of both tools together. Both updated release binaries are installed. Isolated real-Herdr Ctrl+d recovery is verified; the full interactive picker-to-warm-workspace flow remains unverified. Existing workspace selection still only focuses that workspace.

Shell pool is optional: sessionizer requires no pool installation for its picker, ordinary workspace creation, history, or jump-back. Pool-workspace exclusion depends only on the reserved workspace label, not on finding or invoking the pool tool. An executable-level test verifies these behaviors from a standalone deployment with no sibling shell-pool directory.

## Directory configuration

The picker reads only `config.toml` beside the running `herdr-sessionizer` executable every time it opens. It does not use the caller's working directory and ignores `directories.toml`. The shipped file is the editable starting configuration:

```toml
include = ["~/dotfiles", "~/Notes", "~/Downloads", "~/Work", "~/devel", "~/Mail"]
exclude = ["~/devel/Archive", "~/devel/.sync", "~/devel/.SynologyWorkingDirectory"]

[[roots]]
path = "~/VAULT"
depth = 3

[[roots]]
path = "~/devel"
depth = 4
```

`roots` entries require an absolute or `~/` path and a positive `depth`. Optional `status_icons` accepts exactly `"plain"` or `"nerd"`; it defaults to `"plain"` independently of the directory lists. Plain symbols are the default. They are `▶` (working), `✓` (done), `○` (idle), `!` (blocked), and `?` (unknown). Nerd mode uses `` (working), `` (done), `` (idle), and `` (blocked), with plain `?` for unknown, missing, and unexpected statuses. Both the icon and adjacent status text use the status color: working yellow, done green, idle terminal-default, blocked red, unknown/unexpected grey. Status text is unchanged. Nerd mode does not detect fonts or switch icon sets automatically; configure a compatible terminal font. No emojis are used, and icons do not request a larger per-character font size.

Depth one means immediate children of the root, not the root itself. Discovery processes roots in file order, then `include` entries in list order. Paths are normalized without resolving symlinks, and duplicate normalized roots are rejected. A present file replaces the built-in choices completely: omitted lists and an empty file mean no discovered or explicitly included paths. Built-in directory choices are used only when `config.toml` is absent.

`exclude` entries are exact normalized paths. An excluded root is skipped before it is accessed, and an excluded discovered directory prunes its subtree. Explicit `include` entries are processed independently and can re-add an excluded or unavailable path; they are not checked until selection. Discovered symlinks are offered only when they point to existing directories, and are never traversed. Broken links and links to files are omitted. Other discovery errors, including unreadable roots or symlink targets, are shown in the picker.

Invalid configuration and discovery failures remain in the picker header until the picker is dismissed. They do not fall back to defaults, create a workspace, or affect `track`, `jump-back`, or pane-exit recovery. Relative paths, `..` components, unknown keys, invalid types, zero depths, and duplicate normalized roots are errors. Paths containing spaces and shell punctuation are passed as arguments rather than evaluated as shell code.

If the pool executable is absent, ordinary Herdr creation is retained. If it runs but fails, sessionizer stops without retrying creation, since a workspace may already exist. Descriptive Herdr notifications and cold-shell recovery for pool failures are not implemented. Workspace creation itself needs no extra hook or key binding; close-focus recovery requires the `pane.exited` hook described below.

When Ctrl+d exits the final shell in a workspace and Herdr falls back to the pool, the pane-exit hook returns focus to the most recent surviving user workspace. If history has no target, it uses the first remaining user workspace; if none remain, it creates nothing. Ordinary manual pool selection is not redirected. Recovery is asynchronous, so the pool may briefly appear before focus returns. The updated binary and `pane.exited` hook are installed and registered. Future manifest changes need relinking, but no server restart.

## Verification

The runtime requires Herdr and fzf on `PATH`. The Rust test suite passes in debug and release modes. It uses stub tools to check behavior without changing live workspaces:

```sh
cargo test
cargo test --release
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Run the isolated real-Herdr regressions after building:

```sh
python3 tests/live_workspace_exit.py
python3 tests/live_picker.py
python3 tests/live_config_error.py
```

The live Herdr regressions start and stop only named temporary servers with isolated HOME, configuration, state, socket, and plugin paths. They never attach to or modify the default session. Current recorded verification passed all three live scripts; `live_picker.py` passed twice consecutively after waiting for the exact directory target to be ready before acceptance. `live_workspace_exit.py` verifies Ctrl+d focus recovery. `live_picker.py` uses real fzf in a PTY and verifies search-driven nested-agent selection, ANSI output, focus moving to the expected workspace and agent pane across a created tab, paths containing spaces, and picker recovery after an injected agent-focus failure. The picker test also records and checks the live agent metadata shape, accepting `display_name`/`name` and `agent`/`type` variants.

The interactive checks are intended for Herdr 0.9.5, fzf 0.74.5, and macOS. The failure case injects a deterministic `agent focus` failure through an isolated wrapper so it does not depend on timing or destroy the disposable agent. In this isolated scenario, it checks that fzf displays the error and that selecting a workspace row recovers; the Rust stub tests cover stale/missing IDs and dismissal. `live_config_error.py` uses real fzf with stub Herdr and an isolated executable deployment to verify configuration-error visibility, normal dismissal, pane cleanup, and temporary-file cleanup. It uses `pty.fork` so the child receives a controlling terminal and fails if dismissal or cleanup cannot be verified. Linux and other Herdr/fzf versions remain unverified.

The original Bash scripts and Python tests are retained under `legacy/` as historical references. The plugin and current test suite do not use them. The former `directories.toml` layout is likewise historical: only the plugin-local `config.toml` is read.
