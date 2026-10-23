"""Verify real fzf keeps a discovery error visible until dismissal.

This test uses a stub Herdr command and a disposable executable deployment. It
never starts Herdr or changes a live session. Run after ``cargo build --release``::

    python3 tests/live_config_error.py
"""

from __future__ import annotations

import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import tempfile
import termios
import time
import signal
import faulthandler


faulthandler.dump_traceback_later(15, repeat=False)

PROJECT = Path(__file__).resolve().parents[1]
BINARY = PROJECT / "target/release/herdr-sessionizer"
if not BINARY.is_file() or not shutil.which("fzf"):
    raise SystemExit("a release binary and real fzf are required")


with tempfile.TemporaryDirectory(prefix="sessionizer-config-error ", dir="/tmp") as name:
    root = Path(name)
    home = root / "home"
    deployment = root / "plugin"
    command_bin = root / "bin"
    scratch = root / "tmp"
    (home / "VAULT").mkdir(parents=True)
    deployment.mkdir()
    command_bin.mkdir()
    scratch.mkdir()
    shutil.copy2(BINARY, deployment / "herdr-sessionizer")
    # An invalid file must be shown, not replaced with built-in defaults.
    (deployment / "directories.toml").write_text("unknown = true\n")
    herdr = command_bin / "herdr"
    herdr.write_text(
        "#!/bin/sh\n"
        "case \"$1 $2\" in\n"
        "  'workspace list') printf '%s\\n' '{\"result\":{\"workspaces\":[]}}' ;;\n"
        "  'pane list') printf '%s\\n' '{\"result\":{\"panes\":[]}}' ;;\n"
        "  'agent list') printf '%s\\n' '{\"result\":{\"agents\":[]}}' ;;\n"
        "  'status server') exit 0 ;;\n"
        "  'pane close') printf '%s\\n' \"$3\" > \"$FIXTURE/closed-pane\" ;;\n"
        "  *) exit 1 ;;\n"
        "esac\n"
    )
    herdr.chmod(0o755)
    env = os.environ.copy()
    env.update(
        HOME=str(home),
        XDG_STATE_HOME=str(root / "state"),
        TMPDIR=str(scratch),
        FIXTURE=str(root),
        PATH=f"{command_bin}:{env['PATH']}",
        TERM="xterm-256color",
        FZF_DEFAULT_OPTS="--bind=esc:abort",
        HERDR_SESSIONIZER_PANE_ID="picker-pane",
    )

    pid, master = pty.fork()
    if pid == 0:
        os.chdir(root)
        os.execve(
            str(deployment / "herdr-sessionizer"),
            [str(deployment / "herdr-sessionizer"), "pick"],
            env,
        )
        raise AssertionError("execve returned")
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    output = bytearray()
    status = None
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and b"unknown field" not in output:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    output.extend(os.read(master, 65536))
                except OSError:
                    break
        if b"Directory discovery failed" not in output or b"unknown field" not in output:
            raise AssertionError(f"discovery error was not visible in real fzf: {bytes(output)!r}")
        # Dismiss the real fzf after the error is visible. The specification
        # permits a nonzero normal cancellation status.
        os.write(master, b"\x1b")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            waited, status_value = os.waitpid(pid, os.WNOHANG)
            if waited:
                status = status_value
                break
            time.sleep(0.05)
        else:
            os.write(master, b"q")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                waited, status_value = os.waitpid(pid, os.WNOHANG)
                if waited:
                    status = status_value
                    break
                time.sleep(0.05)
            else:
                raise AssertionError("picker dismissal did not complete after Escape and q")
        return_code = os.waitstatus_to_exitcode(status)
        if return_code == 0:
            raise AssertionError("interrupt dismissal unexpectedly returned zero")
        if (root / "closed-pane").read_text() != "picker-pane\n":
            raise AssertionError("picker pane was not closed after dismissal")
        if any(scratch.iterdir()):
            raise AssertionError(f"temporary picker files remain: {list(scratch.iterdir())}")
    finally:
        os.close(master)
        if status is None:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(pid, 0)

    # Repeat the same controlling-terminal check for a deterministic root
    # discovery failure, rather than only a TOML parse failure.
    (home / "loop").symlink_to("loop")
    (deployment / "directories.toml").write_text(
        '[[roots]]\npath = "~/loop"\ndepth = 1\n'
    )
    pid, master = pty.fork()
    if pid == 0:
        os.chdir(root)
        os.execve(
            str(deployment / "herdr-sessionizer"),
            [str(deployment / "herdr-sessionizer"), "pick"],
            env,
        )
        raise AssertionError("execve returned")
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    root_status = None
    root_output = bytearray()
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and b"Directory discovery failed" not in root_output:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    root_output.extend(os.read(master, 65536))
                except OSError:
                    break
        if b"Directory discovery failed" not in root_output or b"loop" not in root_output:
            raise AssertionError(f"root discovery error was not visible: {bytes(root_output)!r}")
        os.write(master, b"\x1b")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            waited, status_value = os.waitpid(pid, os.WNOHANG)
            if waited:
                root_status = status_value
                break
            time.sleep(0.05)
        else:
            os.write(master, b"q")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                waited, status_value = os.waitpid(pid, os.WNOHANG)
                if waited:
                    root_status = status_value
                    break
                time.sleep(0.05)
            else:
                raise AssertionError("root-error dismissal did not complete after Escape and q")
        if os.waitstatus_to_exitcode(root_status) == 0:
            raise AssertionError("root-error dismissal unexpectedly returned zero")
        if (root / "closed-pane").read_text() != "picker-pane\n":
            raise AssertionError("picker pane was not closed after root-error dismissal")
        if any(scratch.iterdir()):
            raise AssertionError(f"temporary picker files remain after root error: {list(scratch.iterdir())}")
    finally:
        os.close(master)
        if root_status is None:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(pid, 0)

print("PASS: real fzf config/root discovery errors, dismissal, and cleanup")
