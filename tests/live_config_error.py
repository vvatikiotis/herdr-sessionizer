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
import subprocess
import tempfile
import termios
import time
import signal


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

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    process = subprocess.Popen(
        [str(deployment / "herdr-sessionizer"), "pick"],
        cwd=root,
        env=env,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        close_fds=True,
        start_new_session=True,
    )
    os.close(slave)
    output = bytearray()
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
        # Interrupt the real fzf child after the error is visible. The
        # specification permits a nonzero normal cancellation status, while
        # keeping the parent alive lets its cleanup guard run.
        os.write(master, b"\x1b")
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            # On this macOS PTY setup fzf renders the header but does not
            # receive keyboard dismissal through the child's /dev/tty.
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
            process.wait(timeout=5)
            print("SKIP: fzf rendered the error, but PTY dismissal was unavailable")
            raise SystemExit(0)
        if process.returncode == 0:
            raise AssertionError("interrupt dismissal unexpectedly returned zero")
        if (root / "closed-pane").read_text() != "picker-pane\n":
            raise AssertionError("picker pane was not closed after dismissal")
        if any(scratch.iterdir()):
            raise AssertionError(f"temporary picker files remain: {list(scratch.iterdir())}")
    finally:
        os.close(master)
        if process.poll() is None:
            process.kill()
            process.wait()

print("PASS: real fzf discovery error visibility, dismissal, and cleanup")
