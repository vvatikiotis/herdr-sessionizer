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
    os.tcsetpgrp(slave, os.getpgrp())


def read_screen(master: int, process: subprocess.Popen[bytes], screen: bytearray, deadline: float) -> None:
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 0.1)
        if not ready:
            continue
        try:
            screen.extend(os.read(master, 65536))
        except OSError:
            return
        if process.poll() is not None:
            return


def child_pids(process: subprocess.Popen[bytes]) -> list[str]:
    result = subprocess.run(
        ["ps", "-axo", "pid=,ppid="],
        check=True,
        capture_output=True,
        text=True,
    )
    return [
        pid
        for line in result.stdout.splitlines()
        for pid, parent in [line.split()]
        if parent == str(process.pid)
    ]


def assert_cleanup(root: Path) -> None:
    assert (root / "closed-pane").read_text() == "picker-pane\n"
    scratch = root / "tmp"
    assert not any(scratch.iterdir()), list(scratch.iterdir())


def assert_no_navigation(root: Path, before: int) -> None:
    calls = (root / "herdr-calls").read_text().splitlines()
    navigation = [
        line for line in calls[before:]
        if line.startswith(("workspace focus", "workspace create", "tab focus", "agent focus"))
    ]
    assert not navigation, navigation


def run_view(root: Path, deployment: Path, env: dict[str, str], config: str, key: bytes) -> None:
    (deployment / "config.toml").write_text(config)
    before_calls = len((root / "herdr-calls").read_text().splitlines()) if (root / "herdr-calls").exists() else 0
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    process = subprocess.Popen(
        [str(deployment / "herdr-sessionizer"), "pick"],
        cwd=root,
        env=env,
        stdin=slave,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        close_fds=True,
        preexec_fn=lambda: controlling_tty(slave),
    )
    os.close(slave)
    screen = bytearray()
    try:
        deadline = time.monotonic() + 10
        while (b"Directory discovery failed" not in screen and b"Configuration" not in screen
               and time.monotonic() < deadline):
            read_screen(master, process, screen, min(deadline, time.monotonic() + 0.1))
            if process.poll() is not None:
                break
        assert b"Directory discovery failed" in screen or b"Configuration" in screen, bytes(screen)
        assert b"\x1b[?1049h" in screen, bytes(screen)
        assert process.poll() is None, f"parent exited before dismissal: {bytes(screen)!r}"
        assert child_pids(process), "fzf child was not alive before dismissal"
        time.sleep(0.5)
        os.write(master, key)
        deadline = time.monotonic() + 5
        while process.poll() is None and time.monotonic() < deadline:
            read_screen(master, process, screen, min(deadline, time.monotonic() + 0.1))
        assert process.poll() is not None, "dismissal timed out after draining terminal output"
        stdout, stderr = process.communicate(timeout=1)
        assert process.returncode != 0
        assert stdout == b"", stdout
        assert b"Directory discovery failed" in stderr, stderr
        assert_no_navigation(root, before_calls)
        assert_cleanup(root)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        os.close(master)


def run_launch_failure(root: Path, deployment: Path, env: dict[str, str]) -> None:
    (deployment / "config.toml").write_text("unknown = true\n")
    before_calls = len((root / "herdr-calls").read_text().splitlines()) if (root / "herdr-calls").exists() else 0
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    process = subprocess.Popen(
        [str(deployment / "herdr-sessionizer"), "pick"],
        cwd=root,
        env=env,
        stdin=slave,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        close_fds=True,
        preexec_fn=lambda: controlling_tty(slave),
    )
    os.close(slave)
    try:
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode != 0
        assert stdout == b"", stdout
        assert b"Directory discovery failed" in stderr, stderr
        assert b"error view exited with status" in stderr, stderr
        assert b"fzf-launch-failed" in stderr, stderr
        assert_no_navigation(root, before_calls)
        assert_cleanup(root)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
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
        "printf '%s\\n' \"$*\" >> \"$FIXTURE/herdr-calls\"\n"
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
        ('[[roots]]\npath = "~/loop"\ndepth = 1\n', b"\x03"),
        ('[[roots]]\npath = "~/loop"\ndepth = 1\n', b"\n"),
    ]
    (home / "loop").symlink_to("loop")
    for config, key in cases:
        print(f"Checking error dismissal with {key!r}: {config.splitlines()[0]}", flush=True)
        (root / "closed-pane").unlink(missing_ok=True)
        run_view(root, deployment, env, config, key)

    failing_fzf = command_bin / "fzf"
    failing_fzf.write_text("#!/bin/sh\nprintf '%s\\n' fzf-launch-failed >&2\nexit 42\n")
    failing_fzf.chmod(0o755)
    (root / "closed-pane").unlink(missing_ok=True)
    run_launch_failure(root, deployment, env)

print("PASS: real fzf error visibility, dismissal, status, output, launch failure, and cleanup")
