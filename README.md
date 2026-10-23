# Herdr Sessionizer

Build the plugin binary before enabling or updating the plugin:

```sh
cargo build --release &&
cp target/release/herdr-sessionizer ./herdr-sessionizer.new &&
chmod 755 ./herdr-sessionizer.new &&
mv -f ./herdr-sessionizer.new ./herdr-sessionizer
```

The temporary-file-and-rename sequence avoids overwriting a running executable. Relink after changing the manifest so Herdr reads its current event hooks:

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

If the pool executable is absent, ordinary Herdr creation is retained. If it runs but fails, sessionizer stops without retrying creation, since a workspace may already exist. Descriptive Herdr notifications and cold-shell recovery for pool failures are not implemented. Workspace creation itself needs no extra hook or key binding; close-focus recovery requires the `pane.exited` hook described below.

When Ctrl+d exits the final shell in a workspace and Herdr falls back to the pool, the pane-exit hook returns focus to the most recent surviving user workspace. If history has no target, it uses the first remaining user workspace; if none remain, it creates nothing. Ordinary manual pool selection is not redirected. Recovery is asynchronous, so the pool may briefly appear before focus returns. The updated binary and `pane.exited` hook are installed and registered. Future manifest changes need relinking, but no server restart.

## Verification

The runtime requires Herdr and fzf on `PATH`. All 20 Rust tests pass in debug and release modes. They use stub tools to check behavior without changing live workspaces:

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
```

Both scripts start and stop only named temporary servers with isolated HOME, configuration, state, socket, and plugin paths. They never attach to or modify the default session. `live_workspace_exit.py` verifies Ctrl+d focus recovery. `live_picker.py` uses real fzf in a PTY and verifies search-driven nested-agent selection, ANSI output, cross-workspace/tab/pane focus, shell-sensitive temporary paths, and a visible retry after an injected agent-focus failure. The picker test also records and checks the live agent metadata shape, accepting `display_name`/`name` and `agent`/`type` variants.

The interactive checks passed on Herdr 0.9.5, fzf 0.74.5, and macOS. The failure case injects a deterministic `agent focus` failure through an isolated wrapper so it does not depend on timing or destroy the disposable agent. It proves fzf error visibility and recovery, while the Rust stub tests cover stale/missing IDs and dismissal. Linux and other Herdr/fzf versions remain unverified.

The original Bash scripts and Python tests are retained under `legacy/` for reference. The plugin and current test suite do not use them.
