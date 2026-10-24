# Herdr Sessionizer

Herdr Sessionizer adds a workspace picker, jump-back command, and agent status in the picker. It tracks workspace visits and can optionally use the neighboring shell-pool tool to open a new directory in a warm shell.

## Build and install

Build and safely replace the binary in this plugin directory:

```sh
cargo build --release &&
cp target/release/herdr-sessionizer ./herdr-sessionizer.new &&
chmod 755 ./herdr-sessionizer.new &&
mv -f ./herdr-sessionizer.new ./herdr-sessionizer
```

Keep `config.toml` beside the binary. For a separate plugin directory, install the binary and copy the default config only when one is not already present:

```sh
install -m 755 target/release/herdr-sessionizer "$PLUGIN_DIR/herdr-sessionizer"
if [ ! -e "$PLUGIN_DIR/config.toml" ]; then
  install -m 644 config.toml "$PLUGIN_DIR/config.toml"
fi
```

Enable and link the plugin for initial setup. Relink after changing `herdr-plugin.toml` so Herdr reads the updated hooks:

```sh
herdr plugin link . --enabled
```

Use these commands to open the picker or return to the previous workspace:

```sh
./herdr-sessionizer pick
./herdr-sessionizer jump-back
```

The shell-pool tool is optional. Without it, the picker, ordinary workspace creation, history, and jump-back still work. If the tool runs but fails, the plugin stops rather than risk creating a duplicate workspace. Pool-workspace exclusion uses its reserved label and does not require the tool to be installed.

## Directory and status settings

Edit the plugin-local `config.toml` to choose directories. This file is read beside the executable each time the picker opens, and a present file replaces the built-in directory choices:

```toml
include = ["~/dotfiles", "~/Notes", "~/Downloads", "~/Work", "~/devel"]
exclude = ["~/devel/Archive"]

[[roots]]
path = "~/VAULT"
depth = 3

[[roots]]
path = "~/devel"
depth = 4

status_icons = "plain" # or "nerd"
```

Root paths must be absolute or start with `~/`, and depth must be positive. Depth 1 means immediate child directories. `include` paths are added directly; `exclude` paths are omitted from discovery. The default status style is `plain`; `nerd` requires a compatible terminal font and is not selected automatically.

Only `config.toml` is read. If upgrading from the old `directories.toml` layout, manually transfer its settings; there is no automatic migration or fallback. Invalid configuration or directory discovery errors appear in the picker and do not fall back to defaults or create a workspace.

## Tests

Run the Rust checks with:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

See [SPEC.md](SPEC.md) and the test suite for implementation and coverage details. Optional live Herdr checks are in `tests/` and use isolated temporary environments.
