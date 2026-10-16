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

Herdr runs `./herdr-sessionizer track` at startup and after workspace-focus events. User-facing commands are:

```sh
./herdr-sessionizer pick
./herdr-sessionizer jump-back
```

For unopened directories, sessionizer source now optionally calls `herdr-shell-pool/herdr-shell-pool-bin` beside its own plugin directory with `new-workspace --cwd <directory> --label <basename>`. Build and install compatible updated versions of both tools together. Both updated release binaries are now installed; live workspace integration still needs verification. Existing workspace selection still only focuses that workspace.

If the pool executable is absent, ordinary Herdr creation is retained. If it runs but fails, sessionizer stops without retrying creation, since a workspace may already exist. Descriptive Herdr notifications and cold-shell recovery for pool failures are not implemented in this first slice. No plugin manifest or key-binding changes are needed.

The runtime requires Herdr and fzf on `PATH`. Rust tests use stub Herdr and fzf programs to check process behavior without changing live workspaces. Tests with real Herdr and fzf are deferred until after the initial implementation.

The original Bash scripts and Python tests are retained under `legacy/` for reference. The plugin and current test suite do not use them.
