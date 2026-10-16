# Herdr Sessionizer

Build the plugin binary before enabling or updating the plugin:

```sh
cargo build --release
cp target/release/herdr-sessionizer ./herdr-sessionizer
```

If Herdr has an older local-plugin registration, relink it after the build so it reads the updated manifest:

```sh
herdr plugin link --enabled .
```

Herdr runs `./herdr-sessionizer track` at startup and after workspace-focus events, and `after-pane-exit` on pane-exit events. The reserved shell-pool workspace is excluded from visit history and jump-back, even if focused. A history refresh also removes previously saved pool entries, including legacy current/previous state. User-facing commands are:

```sh
./herdr-sessionizer pick
./herdr-sessionizer jump-back
```

For unopened directories, sessionizer source now optionally calls `herdr-shell-pool/herdr-shell-pool-bin` beside its own plugin directory with `new-workspace --cwd <directory> --label <basename>`. Build and install compatible updated versions of both tools together. Both updated release binaries are now installed; live workspace integration still needs verification. Existing workspace selection still only focuses that workspace.

Shell pool is optional: sessionizer requires no pool installation for its picker, ordinary workspace creation, history, or jump-back. Pool-workspace exclusion depends only on the reserved workspace label, not on finding or invoking the pool tool. An executable-level test verifies these behaviors from a standalone deployment with no sibling shell-pool directory.

If the pool executable is absent, ordinary Herdr creation is retained. If it runs but fails, sessionizer stops without retrying creation, since a workspace may already exist. Descriptive Herdr notifications and cold-shell recovery for pool failures are not implemented in this first slice. No plugin manifest or key-binding changes are needed.

When Ctrl+d exits the final shell in a workspace and Herdr falls back to the pool, the pane-exit hook returns focus to the most recent surviving user workspace. If history has no target, it uses the first remaining user workspace; if none remain, it creates nothing. Ordinary manual pool selection is not redirected. Relink the plugin after updating the manifest so the new `pane.exited` hook is registered; no server restart is needed.

The runtime requires Herdr and fzf on `PATH`. Rust tests use stub Herdr and fzf programs to check process behavior without changing live workspaces. Run the optional real-Herdr shell-exit regression after building:

```sh
python3 tests/live_workspace_exit.py
```

It starts and stops only its own temporary server with isolated home, configuration, state, and plugin registry. It verifies Ctrl+d focus recovery without requiring a shell-pool installation. Real fzf integration remains deferred.

The original Bash scripts and Python tests are retained under `legacy/` for reference. The plugin and current test suite do not use them.
