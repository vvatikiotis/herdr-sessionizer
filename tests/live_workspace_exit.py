"""Opt-in regression for Ctrl+d focus fallback, using only a disposable Herdr server.

Run after `cargo build --release`:
    python3 tests/live_workspace_exit.py
No live user session, plugin registry, or workspace is modified.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


parser = argparse.ArgumentParser(description=__doc__)
project = Path(__file__).resolve().parents[1]
parser.add_argument("--binary", type=Path, default=project / "target/release/herdr-sessionizer")
args = parser.parse_args()
herdr = shutil.which("herdr")
if not herdr or not args.binary.is_file():
    parser.error("Herdr and a built sessionizer binary are required")

with tempfile.TemporaryDirectory(prefix="hs-exit-", dir="/tmp") as temporary:
    root = Path(temporary)
    session = "workspace-exit-test"
    config = root / "config/herdr/config.toml"
    config.parent.mkdir(parents=True)
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/bash"\n')
    home = root / "home"
    home.mkdir()
    plugin = root / "plugins/sessionizer"
    plugin.mkdir(parents=True)
    shutil.copy2(args.binary, plugin / "herdr-sessionizer")
    shutil.copy2(project / "herdr-plugin.toml", plugin / "herdr-plugin.toml")
    sock = root / "config/herdr/sessions" / session / "herdr.sock"
    env = {key: value for key, value in os.environ.items() if not key.startswith("HERDR_")}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(root / "config"),
               XDG_STATE_HOME=str(root / "state"), HERDR_CONFIG_PATH=str(config),
               HERDR_SOCKET_PATH=str(sock), HERDR_SESSION=session)

    def cli(*arguments):
        result = subprocess.run([herdr, "--session", session, *arguments], env=env,
                                cwd=root, capture_output=True, text=True, timeout=15)
        if result.returncode:
            raise RuntimeError(f"{arguments}: {result.stderr.strip()}")
        return json.loads(result.stdout) if result.stdout.strip() else {}

    def snapshot():
        data = cli("api", "snapshot")
        return data.get("result", data).get("snapshot", data.get("snapshot", data))

    def track():
        subprocess.run([str(plugin / "herdr-sessionizer"), "track"], env=env,
                       cwd=root, check=True, capture_output=True, timeout=10)

    with (root / "server.log").open("w+") as log:
        server = subprocess.Popen([herdr, "--session", session, "server"], env=env,
                                  cwd=root, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 10
            while not sock.exists():
                if server.poll() is not None or time.monotonic() > deadline:
                    log.seek(0)
                    raise RuntimeError("isolated server did not start: " + log.read()[-1500:])
                time.sleep(.05)
            original = cli("workspace", "create", "--cwd", str(home), "--label",
                           "original", "--focus")["result"]["workspace"]["workspace_id"]
            cli("plugin", "link", str(plugin), "--enabled")
            track()
            # Native close fallback also fails for cold-created workspaces. This
            # reserved-label workspace is the minimal stand-in for the shell pool;
            # the reproduction deliberately needs no shell-pool installation.
            cli("workspace", "create", "--cwd", str(home), "--label",
                "__herdr_shell_pool__", "--no-focus")
            created = cli("workspace", "create", "--cwd", str(home), "--label",
                          "temporary-project", "--focus")["result"]
            workspace = created["workspace"]["workspace_id"]
            pane = created["root_pane"]["pane_id"]
            track()
            marker = "SESSIONIZER_EXIT_READY"
            escaped = "".join(f"\\{byte:03o}" for byte in marker.encode())
            cli("pane", "run", pane, "printf '%b\\n' '" + escaped + "'")
            cli("pane", "wait-output", pane, "--match", marker, "--timeout", "5000")
            cli("pane", "send-keys", pane, "ctrl+d")
            deadline = time.monotonic() + 5
            while True:
                after = snapshot()
                closed = not any(w["workspace_id"] == workspace for w in after["workspaces"])
                focused = [w for w in after["workspaces"] if w.get("focused")]
                if closed and any(w["workspace_id"] == original for w in focused):
                    break
                if time.monotonic() > deadline:
                    raise AssertionError(f"after Ctrl+d: closed={closed}, focused={[w.get('label') for w in focused]}")
                time.sleep(.05)
            print("PASS: Ctrl+d returns to the original user workspace, not the pool")
        finally:
            if server.poll() is None:
                try:
                    subprocess.run([herdr, "--session", session, "server", "stop"],
                                   env=env, cwd=root, capture_output=True, timeout=5)
                finally:
                    try:
                        server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        server.terminate()
                        server.wait(timeout=5)
