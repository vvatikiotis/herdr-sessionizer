"""Verify real-fzf configuration and discovery error dismissal.

Run after ``cargo build --release``. The test uses isolated files and a
controlling PTY; it never starts Herdr or changes a live session.
"""

from __future__ import annotations

import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import tempfile
import termios
import time

PROJECT = Path(__file__).resolve().parents[1]
BINARY = PROJECT / "target/release/herdr-sessionizer"
if not BINARY.is_file() or not shutil.which("fzf"):
    raise SystemExit("a release binary and real fzf are required")


def controlling_tty(slave: int) -> None:
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)


def run_case(root: Path, deployment: Path, env: dict[str, str], config: str, key: bytes) -> None:
    (deployment / "directories.toml").write_text(config)
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
    )
    os.close(slave)
    screen = bytearray()
    try:
        deadline = time.monotonic() + 10
        while b"Directory discovery failed" not in screen:
            if time.monotonic() >= deadline:
                raise AssertionError(f"error view did not render: {bytes(screen)!r}")
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                    screen.extend(chunk)
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b[?2004$p" in chunk:
                        os.write(master, b"\x1b[?2004;1$y")
                except OSError as error:
                    raise AssertionError(f"PTY closed before error rendered: {error}") from error
        time.sleep(0.2)
        os.write(master, key)
        deadline = time.monotonic() + 5
        while process.poll() is None and time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                    screen.extend(chunk)
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b[?2004$p" in chunk:
                        os.write(master, b"\x1b[?2004;1$y")
                except OSError:
                    break
        if process.poll() is None:
            raise AssertionError(f"dismissal did not complete for key {key!r}")
        if process.returncode == 0:
            raise AssertionError(f"dismissal returned success for key {key!r}")
        if b"Directory discovery failed" not in screen:
            raise AssertionError(f"original error missing from terminal output: {bytes(screen)!r}")
        closed = root / "closed-pane"
        if closed.read_text() != "picker-pane\n":
            raise AssertionError(f"picker pane was not closed: {closed.read_text()!r}")
        scratch = root / "tmp"
        if any(scratch.iterdir()):
            raise AssertionError(f"temporary picker files remain: {list(scratch.iterdir())}")
    finally:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
        os.close(master)


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
        HERDR_SESSIONIZER_PANE_ID="picker-pane",
        FZF_DEFAULT_OPTS="",
    )

    cases = [
        ("unknown = true\n", b"\x1b"),
        ("unknown = true\n", b"\x03"),
        ("unknown = true\n", b"\n"),
        ('[[roots]]\npath = "~/loop"\ndepth = 1\n', b"\x1b"),
    ]
    (home / "loop").symlink_to("loop")
    for index, (config, key) in enumerate(cases):
        (root / "closed-pane").unlink(missing_ok=True)
        run_case(root, deployment, env, config, key)

print("PASS: real fzf config/discovery error visibility, dismissal, status, output, and cleanup")
