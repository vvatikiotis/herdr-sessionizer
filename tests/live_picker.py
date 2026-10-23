"""Opt-in interactive picker regression using real fzf and a disposable Herdr server.

Run after ``cargo build --release``::

    python3 tests/live_picker.py

The test creates a named Herdr server with isolated HOME/config/state/socket
paths. It never attaches to or mutates the user's default session.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import struct
import shutil
import select
import termios
import signal
import subprocess
import tempfile
import time


PROJECT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, default=PROJECT / "target/release/herdr-sessionizer")
args = parser.parse_args()
herdr = shutil.which("herdr")
fzf = shutil.which("fzf")
if not herdr or not fzf or not args.binary.is_file():
    parser.error("Herdr, fzf, and a built sessionizer binary are required")


def wait_for(predicate, description: str, timeout: float = 10.0):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline:
            raise RuntimeError(f"timed out waiting for {description}")
        time.sleep(0.05)


with tempfile.TemporaryDirectory(prefix="hs-picker space-", dir="/tmp") as temporary:
    root = Path(temporary)
    session = "sessionizer-picker-test"
    config = root / "config/herdr/config.toml"
    config.parent.mkdir(parents=True)
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/bash"\n')
    home = root / "home"
    alpha = home / "VAULT/alpha"
    beta = home / "VAULT/beta"
    alpha.mkdir(parents=True)
    beta.mkdir(parents=True)
    (home / "devel").mkdir()
    plugin = root / "plugins/sessionizer"
    plugin.mkdir(parents=True)
    command_bin = root / "bin"
    command_bin.mkdir()
    (command_bin / "herdr").write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$*\" >> \"$FIXTURE/herdr-wrapper.log\"\n"
        "if [ \"$1 $2\" = \"agent focus\" ] && [ -f \"$FIXTURE/fail-agent-focus\" ]; then exit 1; fi\n"
        "exec \"$REAL_HERDR\" \"$@\"\n"
    )
    (command_bin / "herdr").chmod(0o755)
    shutil.copy2(args.binary, plugin / "herdr-sessionizer")
    shutil.copy2(PROJECT / "herdr-plugin.toml", plugin / "herdr-plugin.toml")
    socket = root / "config/herdr/sessions" / session / "herdr.sock"
    env = {key: value for key, value in os.environ.items() if not key.startswith("HERDR_")}
    env.update(
        HOME=str(home),
        XDG_CONFIG_HOME=str(root / "config"),
        XDG_STATE_HOME=str(root / "state"),
        HERDR_CONFIG_PATH=str(config),
        HERDR_SOCKET_PATH=str(socket),
        HERDR_SESSION=session,
        TERM="xterm-256color",
        FIXTURE=str(root),
        REAL_HERDR=herdr,
        PATH=f"{command_bin}:{os.environ['PATH']}",
    )

    def cli(*arguments: str):
        result = subprocess.run(
            [herdr, "--session", session, *arguments],
            env=env,
            cwd=root,
            capture_output=True,
            text=True,
            timeout=20,
        )
        if result.returncode:
            raise RuntimeError(f"{arguments}: {result.stderr.strip()}")
        return json.loads(result.stdout) if result.stdout.strip() else {}

    def snapshot():
        data = cli("api", "snapshot")
        return data.get("result", data).get("snapshot", data.get("snapshot", data))

    def read_pty(master, process, output, deadline, stop=None):
        while process.poll() is None and time.monotonic() < deadline:
            if stop and stop(output):
                break
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    output.extend(os.read(master, 65536))
                except OSError:
                    break

    def run_picker(*input_chunks: bytes, wait_for_text: bytes = b"", timeout: float = 15.0):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        process = subprocess.Popen(
            [str(plugin / "herdr-sessionizer"), "pick"],
            env=env,
            cwd=root,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            close_fds=True,
        )
        os.close(slave)
        output = bytearray()
        try:
            deadline = time.monotonic() + timeout
            read_pty(
                master,
                process,
                output,
                deadline,
                (lambda data: wait_for_text in data) if wait_for_text else None,
            )
            if wait_for_text and wait_for_text not in output:
                raise RuntimeError(f"picker did not render the requested state: {bytes(output)[-3000:]!r}")
            if process.poll() is not None:
                raise RuntimeError(f"picker exited before input (status {process.returncode}): {bytes(output)[-3000:]!r}")
            for chunk in input_chunks:
                try:
                    os.write(master, chunk)
                except OSError as error:
                    raise RuntimeError(f"picker input failed: {error}; output={bytes(output)[-3000:]!r}") from error
            read_pty(master, process, output, deadline)
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                process.wait(timeout=3)
            return process.returncode, bytes(output)
        finally:
            os.close(master)
            if process.poll() is None:
                process.kill()
                process.wait()

    with (root / "server.log").open("w+") as log:
        server = subprocess.Popen([herdr, "--session", session, "server"], env=env, cwd=root, stdout=log, stderr=log)
        try:
            wait_for(socket.exists, "isolated Herdr socket")
            alpha_workspace = cli("workspace", "create", "--cwd", str(alpha), "--label", "alpha", "--focus")["result"]
            beta_workspace = cli("workspace", "create", "--cwd", str(beta), "--label", "beta", "--no-focus")["result"]
            beta_tab = cli("tab", "create", "--workspace", beta_workspace["workspace"]["workspace_id"], "--cwd", str(beta), "--focus")["result"]
            agent_pane = beta_tab["root_pane"]["pane_id"]
            # This is a real Herdr-recognized agent in the disposable server.
            cli("agent", "start", "pickeragent", "--kind", "pi", "--pane", agent_pane, "--timeout", "60000", "--", "--no-session")
            cli("plugin", "link", str(plugin), "--enabled")

            agents = snapshot()["agents"]
            agent = next((item for item in agents if item.get("pane_id") == agent_pane), None)
            if agent is None:
                raise AssertionError(f"Herdr did not report the started agent: {agents}")
            # Audit the live metadata shape used by agent_rows(), including the
            # display_agent/name variants seen across Herdr releases.
            display = agent.get("display_name") or agent.get("name") or agent.get("display_agent")
            kind = agent.get("agent") or agent.get("type")
            search_term = display or kind
            if not search_term:
                raise AssertionError(f"agent has no searchable display/type metadata: {agent}")
            status = agent.get("agent_status") or "unknown"

            code, screen = run_picker(str(search_term).encode() + b"\x1b[B\r", wait_for_text=str(search_term).encode())
            if code != 0:
                raise AssertionError(f"agent selection failed (status {code}): {screen[-2000:]!r}")
            if str(beta).encode() not in screen:
                raise AssertionError(f"agent selection did not print beta directory: {screen[-2000:]!r}")
            focused = snapshot()
            if not any(item.get("workspace_id") == beta_workspace["workspace"]["workspace_id"] and item.get("focused") for item in focused["workspaces"]):
                raise AssertionError("agent selection did not focus the beta workspace")
            if not any(item.get("pane_id") == agent_pane and item.get("focused") for item in focused["panes"]):
                raise AssertionError("agent selection did not focus the agent pane")
            if b"\x1b[" not in screen:
                raise AssertionError("real fzf transcript contained no ANSI styling")
            if f"[{status}]".encode() not in screen:
                raise AssertionError(f"real fzf transcript omitted status text [{status}]: {screen[-3000:]!r}")

            # Keep the picker snapshot while making its captured pane stale.
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
            picker = subprocess.Popen([str(plugin / "herdr-sessionizer"), "pick"], env=env, cwd=root, stdin=slave, stdout=slave, stderr=slave)
            os.close(slave)
            try:
                time.sleep(1)
                (root / "fail-agent-focus").touch()
                os.write(master, str(search_term).encode() + b"\x1b[B\r")
                error_output = bytearray()
                deadline = time.monotonic() + 10
                read_pty(
                    master,
                    picker,
                    error_output,
                    deadline,
                    lambda data: b"Agent selection failed" in data,
                )
                if b"Agent selection failed" not in error_output:
                    raise AssertionError(f"stale target error was not visible in fzf; wrapper={((root / 'herdr-wrapper.log').read_text() if (root / 'herdr-wrapper.log').exists() else '')!r}: {bytes(error_output)[-2000:]!r}")
                # Selecting another row retries the same snapshot and dismisses
                # the error without creating or retargeting an agent.
                os.write(master, b"\x15")
                time.sleep(0.2)
                os.write(master, b"beta")
                time.sleep(0.2)
                os.write(master, b"\r")
                deadline = time.monotonic() + 10
                read_pty(master, picker, error_output, deadline)
                if picker.poll() is None:
                    raise AssertionError(f"picker did not recover after visible selection error: {bytes(error_output)[-3000:]!r}")
                if picker.returncode != 0 or str(beta).encode() not in error_output:
                    raise AssertionError(f"picker did not recover after visible selection error: {bytes(error_output)[-3000:]!r}")
            finally:
                os.close(master)
                if picker.poll() is None:
                    picker.kill()
                    picker.wait()

            print("PASS: real fzf search/ANSI, cross-workspace tab/pane focus, and retryable stale-target error")
        finally:
            if server.poll() is None:
                subprocess.run([herdr, "--session", session, "server", "stop"], env=env, cwd=root, capture_output=True, timeout=5)
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.terminate()
                    server.wait(timeout=5)
