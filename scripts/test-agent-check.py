"""Verify the published helper in disposable git repositories, isolating content checks from Cargo."""

import os
from pathlib import Path
import stat
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
rust_pin = (
    (repo / "rust-toolchain.toml").read_text().split('channel = "')[1].split('"')[0]
)


def command(*args, cwd, env, check=True):
    return subprocess.run(
        args,
        cwd=cwd,
        env=env,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def fixture_bash(env):
    if os.name != "nt":
        return "bash"
    # The Windows system bash.exe may launch WSL. Select the Bash belonging
    # to Git for Windows, including installations outside Program Files.
    git_exec = Path(command("git", "--exec-path", cwd=repo, env=env).stdout.strip())
    for parent in git_exec.parents:
        bash = parent / "usr" / "bin" / "bash.exe"
        if bash.is_file():
            return str(bash)
    raise RuntimeError("Install Git for Windows with Git Bash to run these fixtures.")


# Git also inherits repository selection, index paths and config overrides
# from the environment when invoked indirectly by the Bash helper.
clean_env = {k: v for k, v in os.environ.items() if not k.upper().startswith("GIT_")}
bash = fixture_bash(clean_env)


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
        (work / "tracked").write_bytes(b"original\n")
        (work / ".gitignore").write_bytes(b"/ignored-output\n")
        write_executable(
            tools / "cargo",
            """#!/bin/sh
if [ "$1" = "--version" ]; then echo "cargo $FIXTURE_RUST"; fi
if [ "$1" = "check" ] || { [ "$1" = "clippy" ] && [ "$3" = "--workspace" ]; }; then
  case "$MUTATION" in
    tracked) echo 'new content' > tracked ;;
    untracked) echo 'new content' > loose ;;
    generated) echo 'new content' > generated ;;
    stage) git add tracked ;;
    index)
      blob=$(echo 'new index' | git hash-object -w --stdin) || exit 2
      git update-index --cacheinfo "100644,$blob,tracked" || exit 2 ;;
    commit) git commit --allow-empty -qm checkpoint || exit 2 ;;
    executable) chmod +x tracked || exit 2 ;;
    index-executable)
      blob=$(git rev-parse :tracked) || exit 2
      git update-index --cacheinfo "100755,$blob,tracked" || exit 2 ;;
    fail-write) echo 'failed output' > generated; exit 9 ;;
    ignored) echo 'ignored output' > ignored-output ;;
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
            clean_env,
            HOME=str(base),
            XDG_CONFIG_HOME=str(base / "config"),
            GIT_CONFIG_NOSYSTEM="1",
            GIT_CONFIG_GLOBAL=str(base / "fixture.gitconfig"),
            PATH=str(tools) + os.pathsep + clean_env["PATH"],
            MUTATION=mutation,
            FIXTURE_RUST=rust_pin,
        )

        def git(*args):
            return command("git", *args, cwd=work, env=env).stdout

        git("init", "-q", "--template=")
        git("config", "user.name", "Fixture")
        git("config", "user.email", "fixture@example.invalid")
        git("config", "core.autocrlf", "false")
        # Keep porcelain unchanged for the POSIX executable-bit case.
        git("config", "core.filemode", "false")
        git("add", ".")
        git("commit", "-qm", "fixture")
        tracked = work / "tracked"
        loose = work / "loose"
        nested = work / "nested" / "child" / "note"
        if state in ("staged", "unstaged", "mixed"):
            tracked.write_bytes(b"user edit\n")
        if state in ("staged", "mixed"):
            git("add", "tracked")
        if state == "mixed":
            tracked.write_bytes(b"further edit\n")
        if state == "untracked":
            loose.write_bytes(b"user edit\n")
        if state == "nested":
            nested.parent.mkdir(parents=True)
            nested.write_bytes(b"nested user edit\n")

        start = git("status", "--porcelain")
        head = git("rev-parse", "HEAD")
        index = git("ls-files", "--stage")
        entry = git("ls-files", "--stage", "--", "tracked")
        start_mode = tracked.stat().st_mode
        start_content = {
            path: path.read_bytes() for path in (tracked, loose, nested) if path.exists()
        }
        result = command(
            bash, "scripts/agent-check", mode, cwd=work, env=env, check=False
        )
        end = git("status", "--porcelain")
        assert result.returncode == expected, (
            mode,
            state,
            mutation,
            result.returncode,
            result.stdout,
        )
        if mutation in ("none", "fail", "ignored"):
            assert start == end and git("rev-parse", "HEAD") == head
            assert git("ls-files", "--stage") == index
            assert all(path.read_bytes() == data for path, data in start_content.items())
        else:
            assert "changed during verification" in result.stdout
        if state == "nested":
            assert start == "?? nested/\n" and nested.read_bytes() == b"nested user edit\n"
        if mutation == "index":
            assert start == end == "MM tracked\n"
            assert git("ls-files", "--stage", "--", "tracked") != entry
            assert git("show", ":tracked") == "new index\n"
            assert tracked.read_bytes() == start_content[tracked] == b"further edit\n"
        if mutation == "commit":
            assert start == end == ""
            assert git("rev-parse", "HEAD^") == head
            assert git("rev-parse", "HEAD^{tree}") == git("rev-parse", head.strip() + "^{tree}")
        if mutation == "executable":
            assert start == end == " M tracked\n" and git("ls-files", "--stage") == index
            assert not start_mode & stat.S_IXUSR and tracked.stat().st_mode & stat.S_IXUSR
            assert tracked.read_bytes() == start_content[tracked]
        if mutation == "index-executable":
            assert entry.startswith("100644 ")
            assert git("ls-files", "--stage", "--", "tracked") == "100755" + entry[6:]
            assert tracked.read_bytes() == start_content[tracked]
        if mutation == "fail-write":
            assert end == "?? generated\n"
            assert (work / "generated").read_bytes() == b"failed output\n"
        if mutation == "ignored":
            assert (work / "ignored-output").read_bytes() == b"ignored output\n"
            assert git("check-ignore", "ignored-output") == "ignored-output\n"
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
    check_case(mode, "mixed", "index", 1)
    check_case(mode, "clean", "commit", 1)
    # NTFS exposes the executable bit in Git's index, not Python's file mode.
    mode_change = "index-executable" if os.name == "nt" else "executable"
    check_case(mode, "unstaged", mode_change, 1)
    check_case(mode, "nested", "none", 0)
    check_case(mode, "clean", "fail-write", 1)
    check_case(mode, "clean", "ignored", 0)
