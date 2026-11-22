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


def fixture_environment(base):
    # Repository selectors, alternate indexes, config injection and hooks from
    # the invoking checkout must never reach a disposable Git command.
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("GIT_")}
    env.update(
        HOME=str(base),
        XDG_CONFIG_HOME=str(base / "config"),
        GIT_CONFIG_GLOBAL=str(base / "gitconfig"),
        GIT_CONFIG_NOSYSTEM="1",
        GIT_ATTR_NOSYSTEM="1",
        GIT_TEMPLATE_DIR=str(base / "template"),
    )
    (base / "template").mkdir()
    return env


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



def write_executable(path, code):
    path.write_bytes(code.encode("utf-8"))
    path.chmod(0o755)


def check_case(mode, state, mutation, expected):
    with tempfile.TemporaryDirectory() as temp:
        base = Path(temp)
        env = fixture_environment(base)
        bash = fixture_bash(env)
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
        (work / ".gitignore").write_bytes(b"/ignored\n")
        write_executable(
            tools / "cargo",
            """#!/bin/sh
if [ "$1" = "--version" ]; then echo "cargo $FIXTURE_RUST"; fi
if [ "$1" = "check" ] || [ "$1" = "clippy" -a "$3" = "--workspace" ]; then
  case "$MUTATION" in
    tracked) echo 'new content' > tracked ;;
    untracked) echo 'new content' > loose ;;
    generated) echo 'new content' > generated ;;
    stage) git add tracked ;;
    index) blob=$(printf 'replacement' | git hash-object -w --stdin) &&
           git update-index --cacheinfo "100644,$blob,tracked" || exit 8 ;;
    commit) git commit --allow-empty -qm checkpoint || exit 8 ;;
    mode) chmod +x tracked || exit 8 ;;
    index-mode) blob=$(git rev-parse :tracked) &&
                git update-index --cacheinfo "100755,$blob,tracked" || exit 8 ;;
    nested) printf 'new content' > dir/sub/loose ;;
    fail-write) printf 'new content' > generated; exit 9 ;;
    ignored) printf 'cache' > ignored ;;
    fail) exit 9 ;;
  esac
fi
exit 0
""",
        )
        write_executable(tools / "rustc", '#!/bin/sh\necho "rustc $FIXTURE_RUST"\n')
        write_executable(tools / "node", "#!/bin/sh\necho 20\n")
        write_executable(tools / "npm", "#!/bin/sh\nexit 0\n")
        env.update(
            PATH=str(tools) + os.pathsep + env["PATH"],
            MUTATION=mutation,
            FIXTURE_RUST=rust_pin,
        )

        def git(*args):
            return command("git", *args, cwd=work, env=env).stdout

        git("init", "-q")
        git("config", "user.name", "Fixture")
        git("config", "user.email", "fixture@example.invalid")
        git("add", ".")
        git("commit", "-qm", "fixture")
        tracked = work / "tracked"
        if state in ("staged", "unstaged", "mixed"):
            tracked.write_bytes(b"user edit\n")
        if state in ("staged", "mixed"):
            git("add", "tracked")
        if state == "mixed":
            tracked.write_bytes(b"working edit\n")
        if state == "untracked":
            (work / "loose").write_bytes(b"user edit\n")
        nested = work / "dir" / "sub" / "loose"
        if state == "nested":
            nested.parent.mkdir(parents=True)
            nested.write_bytes(b"nested edit\n")
        status = ("status", "--porcelain", "--untracked-files=all")
        start = git(*status)
        start_head = git("rev-parse", "HEAD")
        start_index = git("ls-files", "--stage")
        start_mode = tracked.stat().st_mode
        if state == "mixed":
            assert git("show", ":tracked") == "user edit\n"
        start_content = {
            f: f.read_bytes() for f in (tracked, work / "loose", nested) if f.exists()
        }
        result = command(
            bash, "scripts/agent-check", mode, cwd=work, env=env, check=False
        )
        end = git(*status)
        assert result.returncode == expected, (
            mode,
            state,
            mutation,
            result.returncode,
            result.stdout,
        )
        if mutation == "commit":
            assert git("rev-parse", "HEAD") != start_head
            assert git("rev-parse", "HEAD^") == start_head
            assert not git("diff", "HEAD^", "HEAD")
        else:
            assert git("rev-parse", "HEAD") == start_head
        if mutation == "index":
            assert start == end == "MM tracked\n"
            assert git("show", ":tracked") == "replacement"
        elif mutation == "index-mode":
            assert git("ls-files", "--stage", "tracked").startswith("100755 ")
            assert git("show", ":tracked") == git("show", "HEAD:tracked")
        elif mutation == "stage":
            assert git("show", ":tracked") == "user edit\n"
        else:
            assert git("ls-files", "--stage") == start_index
        if mutation == "mode":
            assert start == end == " M tracked\n"
            assert not start_mode & stat.S_IXUSR and tracked.stat().st_mode & stat.S_IXUSR
        for path, original in start_content.items():
            if mutation == "tracked" and path == tracked:
                assert path.read_text() == "new content\n"
            elif mutation == "untracked" and path.name == "loose":
                assert path.read_text() == "new content\n"
            elif mutation == "nested" and path == nested:
                assert start == end == "?? dir/sub/loose\n"
                assert path.read_bytes() == b"new content"
            else:
                assert path.read_bytes() == original
        if mutation == "fail-write":
            assert (work / "generated").read_bytes() == b"new content"
        if mutation == "ignored":
            assert (work / "ignored").read_bytes() == b"cache"
        if mutation in ("none", "fail", "ignored", "commit"):
            assert start == end
        assert ("changed during verification" in result.stdout) == (expected == 1)
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
    # Windows chmod cannot set a POSIX executable bit. Exercise its Git index mode.
    check_case(mode, "unstaged", "index-mode" if os.name == "nt" else "mode", 1)
    check_case(mode, "nested", "none", 0)
    check_case(mode, "nested", "nested", 1)
    check_case(mode, "clean", "fail-write", 1)
    check_case(mode, "clean", "ignored", 0)
