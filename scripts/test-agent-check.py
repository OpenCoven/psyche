"""Verify the published helper in disposable git repositories, isolating content checks from Cargo."""

import os
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
rust_pin = (
    (repo / "rust-toolchain.toml").read_text().split('channel = "')[1].split('"')[0]
)


def command(*args, cwd, env=None, check=True):
    return subprocess.run(
        args,
        cwd=cwd,
        env=env,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def fixture_bash():
    if os.name != "nt":
        return "bash"
    # The Windows system bash.exe may launch WSL. Select the Bash belonging
    # to Git for Windows, including installations outside Program Files.
    git_exec = Path(command("git", "--exec-path", cwd=repo).stdout.strip())
    for parent in git_exec.parents:
        bash = parent / "usr" / "bin" / "bash.exe"
        if bash.is_file():
            return str(bash)
    raise RuntimeError("Install Git for Windows with Git Bash to run these fixtures.")


bash = fixture_bash()


def write_executable(path, code):
    path.write_bytes(code.encode("utf-8"))
    path.chmod(0o755)


def check_case(mode, state, mutation, expected):
    with tempfile.TemporaryDirectory() as temp:
        base = Path(temp)
        work = base / "fixture"
        work.mkdir()
        tools = base / "tools"
        tools.mkdir()
        helpers = work / "scripts"
        helpers.mkdir()
        for name in ("agent-check", "agent-bootstrap"):
            content = (repo / "scripts" / name).read_text()
            write_executable(helpers / name, content)
        (helpers / "check-g2-evidence-test.py").write_text("pass\n")
        (helpers / "check-g2-attestation-scope-test.py").write_text("pass\n")
        (work / "rust-toolchain.toml").write_text(
            (repo / "rust-toolchain.toml").read_text()
        )
        (work / "tracked").write_text("original\n")
        write_executable(
            tools / "cargo",
            """#!/bin/sh
if [ "$1" = "--version" ]; then echo "cargo $FIXTURE_RUST"; fi
if [ "$1" = "check" ] || [ "$1" = "clippy" -a "$2" = "--locked" ]; then
  case "$MUTATION" in
    tracked) echo 'new content' > tracked ;;
    untracked) echo 'new content' > loose ;;
    generated) echo 'new content' > generated ;;
    stage) git add tracked ;;
    fail) exit 9 ;;
  esac
fi
exit 0
""",
        )
        write_executable(tools / "rustc", '#!/bin/sh\necho "rustc $FIXTURE_RUST"\n')
        write_executable(tools / "node", "#!/bin/sh\necho 20\n")
        write_executable(tools / "npm", "#!/bin/sh\nexit 0\n")
        env = dict(
            os.environ,
            PATH=str(tools) + os.pathsep + os.environ["PATH"],
            MUTATION=mutation,
            FIXTURE_RUST=rust_pin,
        )
        command("git", "init", "-q", cwd=work)
        command("git", "add", ".", cwd=work)
        command(
            "git",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
            cwd=work,
        )
        if state in ("staged", "unstaged"):
            (work / "tracked").write_text("user edit\n")
        if state == "staged":
            command("git", "add", "tracked", cwd=work)
        if state == "untracked":
            (work / "loose").write_text("user edit\n")
        start = command("git", "status", "--porcelain", cwd=work).stdout
        start_content = {
            f.name: f.read_bytes()
            for f in [work / "tracked", work / "loose"]
            if f.exists()
        }
        result = command(
            bash, "scripts/agent-check", mode, cwd=work, env=env, check=False
        )
        end = command("git", "status", "--porcelain", cwd=work).stdout
        assert result.returncode == expected, (
            mode,
            state,
            mutation,
            result.returncode,
            result.stdout,
        )
        if mutation in ("none", "fail"):
            assert start == end
            assert all((work / k).read_bytes() == v for k, v in start_content.items())
        if mutation not in ("none", "fail"):
            assert "changed during verification" in result.stdout
        print(
            mode,
            state,
            mutation,
            result.returncode,
            repr(start.strip("\n")),
            repr(end.strip("\n")),
        )


for mode in ("fast", "full"):
    for state in ("clean", "staged", "unstaged", "untracked"):
        check_case(mode, state, "none", 0)
    for state, mutation in [
        ("unstaged", "tracked"),
        ("untracked", "untracked"),
        ("clean", "generated"),
        ("unstaged", "stage"),
    ]:
        check_case(mode, state, mutation, 1)
    check_case(mode, "clean", "fail", 9)
