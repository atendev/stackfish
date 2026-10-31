"""Tests for tools/check.sh, tools/install-hooks.sh, the pre-commit hook and the CI files."""

import os
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent
CHECK = TOOLS / "check.sh"
INSTALL = TOOLS / "install-hooks.sh"
HOOK = TOOLS / "hooks" / "pre-commit"

# Each stub logs "<name> <cwd> <args>" and exits with $STUB_EXIT_<NAME>_<FIRST ARG> or 0.
STUB = """#!/bin/sh
echo "{name} $(pwd) $*" >> "$STUB_LOG"
key=$(printf '%s' "STUB_EXIT_{name}_$1" | tr -c 'A-Za-z0-9_' '_')
eval "code=\\${{$key:-0}}"
exit "$code"
"""


class CheckScriptTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        for name in ("cargo", "python3"):
            stub = self.bin / name
            stub.write_text(STUB.format(name=name))
            stub.chmod(0o755)
        self.log = self.tmp / "log"
        self.log.touch()
        self.db_dir = self.tmp / "stackfish-db"
        self.env = {
            "PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}",
            "STUB_LOG": str(self.log),
            "STACKFISH_DB_DIR": str(self.db_dir),
            "HOME": os.environ.get("HOME", str(self.tmp)),
        }

    def run_check(self, *args, cwd=ROOT, **env):
        return subprocess.run(
            [str(CHECK), *args],
            cwd=cwd,
            env={**self.env, **env},
            capture_output=True,
            text=True,
        )

    def calls(self):
        return [line.split(" ", 2) for line in self.log.read_text().splitlines()]

    def test_stubs_shadow_real_tools(self):
        for name in ("cargo", "python3"):
            self.assertEqual(shutil.which(name, path=self.env["PATH"]), str(self.bin / name))

    def test_unknown_argument_exits_2_with_usage(self):
        result = self.run_check("--bogus")
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_failing_fmt_names_the_check_and_stops(self):
        result = self.run_check(STUB_EXIT_cargo_fmt="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fmt", result.stderr)
        self.assertEqual([c[2] for c in self.calls()], ["fmt --all -- --check"])

    def test_fast_mode_runs_fmt_and_clippy_only(self):
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            [c[2] for c in self.calls()],
            ["fmt --all -- --check", "clippy --workspace --all-targets -- -D warnings"],
        )

    def test_full_mode_adds_tests_and_unittest(self):
        result = self.run_check("--full")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            [(c[0], c[2]) for c in self.calls()],
            [
                ("cargo", "fmt --all -- --check"),
                ("cargo", "clippy --workspace --all-targets -- -D warnings"),
                ("cargo", "test --workspace"),
                ("python3", "-m unittest discover -s tools"),
            ],
        )

    def test_full_mode_notes_missing_db(self):
        result = self.run_check("--full")
        self.assertIn("db_scripts_smoke.rs", result.stdout)
        self.assertNotIn("::notice::", result.stdout)

    def test_github_actions_notice_annotation(self):
        result = self.run_check("--full", GITHUB_ACTIONS="true")
        notices = [l for l in result.stdout.splitlines() if "db_scripts_smoke.rs" in l]
        self.assertEqual(len(notices), 1)
        self.assertTrue(notices[0].startswith("::notice::"))

    def test_real_sqlite_header_gives_no_notice(self):
        self.db_dir.mkdir()
        (self.db_dir / "stackfish.db").write_bytes(b"SQLite format 3\0" + b"\0" * 84)
        result = self.run_check("--full")
        self.assertNotIn("db_scripts_smoke.rs", result.stdout)

    def test_lfs_pointer_gives_notice(self):
        self.db_dir.mkdir()
        (self.db_dir / "stackfish.db").write_text(
            "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 1\n"
        )
        result = self.run_check("--full")
        self.assertIn("db_scripts_smoke.rs", result.stdout)

    def test_short_file_gives_notice(self):
        self.db_dir.mkdir()
        (self.db_dir / "stackfish.db").write_bytes(b"SQLite")
        result = self.run_check("--full")
        self.assertIn("db_scripts_smoke.rs", result.stdout)

    def test_fast_mode_has_no_db_notice(self):
        result = self.run_check()
        self.assertNotIn("db_scripts_smoke.rs", result.stdout)

    def test_runs_from_subdirectory_at_repo_root(self):
        result = self.run_check(cwd=ROOT / "crates")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.calls())
        for call in self.calls():
            self.assertEqual(call[1], str(ROOT))


class InstallHooksTest(unittest.TestCase):
    def setUp(self):
        self.repo = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.repo)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "test")
        self.git("config", "commit.gpgsign", "false")
        self.hooks = self.repo / ".git" / "hooks"
        self.hooks.mkdir(exist_ok=True)
        self.lfs = {}
        for name in ("post-checkout", "post-commit", "post-merge", "pre-push"):
            path = self.hooks / name
            path.write_text(f"#!/bin/sh\ngit lfs {name} \"$@\"\n")
            path.chmod(0o755)
            self.lfs[name] = path.read_bytes()

    def git(self, *args, cwd=None):
        return subprocess.run(
            ["git", *args], cwd=cwd or self.repo, capture_output=True, text=True
        )

    def install(self, cwd=None):
        return subprocess.run(
            [str(INSTALL)], cwd=cwd or self.repo, capture_output=True, text=True
        )

    def assert_installed(self):
        dest = self.hooks / "pre-commit"
        self.assertEqual(dest.read_bytes(), HOOK.read_bytes())
        self.assertTrue(os.access(dest, os.X_OK))

    def assert_lfs_untouched(self):
        for name, content in self.lfs.items():
            self.assertEqual((self.hooks / name).read_bytes(), content)
        self.assertEqual(self.git("config", "--get", "core.hooksPath").stdout, "")

    def test_installs_pre_commit_only(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed()
        self.assert_lfs_untouched()

    def test_second_run_is_a_no_op(self):
        self.install()
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed()
        self.assertEqual(list(self.hooks.glob("pre-commit.bak*")), [])

    def test_backs_up_a_different_hook_without_clobbering(self):
        (self.hooks / "pre-commit").write_text("old one\n")
        self.install()
        self.assertEqual((self.hooks / "pre-commit.bak").read_text(), "old one\n")
        (self.hooks / "pre-commit").write_text("old two\n")
        self.install()
        self.assertEqual((self.hooks / "pre-commit.bak").read_text(), "old one\n")
        self.assertEqual((self.hooks / "pre-commit.bak.1").read_text(), "old two\n")
        self.assert_installed()

    def test_runs_from_subdirectory(self):
        sub = self.repo / "a" / "b"
        sub.mkdir(parents=True)
        result = self.install(cwd=sub)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed()

    def test_refuses_when_hooks_path_is_set(self):
        self.git("config", "core.hooksPath", "custom-hooks")
        result = self.install()
        self.assertEqual(result.returncode, 1)
        self.assertIn("custom-hooks", result.stderr)
        self.assertFalse((self.hooks / "pre-commit").exists())

    def commit_with_check_exit(self, code):
        tools = self.repo / "tools"
        tools.mkdir(exist_ok=True)
        stub = tools / "check.sh"
        stub.write_text(f"#!/bin/sh\nexit {code}\n")
        stub.chmod(0o755)
        (self.repo / "file").write_text(str(code))
        self.git("add", "-A")
        return self.git("commit", "-q", "-m", "x")

    def test_failing_check_blocks_commit(self):
        self.install()
        result = self.commit_with_check_exit(1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotEqual(self.git("rev-parse", "--verify", "-q", "HEAD").returncode, 0)

    def test_passing_check_allows_commit(self):
        self.install()
        result = self.commit_with_check_exit(0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("rev-parse", "--verify", "-q", "HEAD").returncode, 0)


class RepoFilesTest(unittest.TestCase):
    def test_scripts_are_executable(self):
        for path in (CHECK, INSTALL, HOOK):
            self.assertTrue(os.access(path, os.X_OK), path)

    def test_toolchain_is_pinned(self):
        toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]
        self.assertEqual(toolchain["channel"], "1.98.0")
        self.assertLessEqual({"rustfmt", "clippy"}, set(toolchain["components"]))

    def test_ci_runs_full_check_on_master(self):
        text = (ROOT / ".github" / "workflows" / "ci.yml").read_text()
        self.assertIn("branches: [master]", text)
        self.assertIn("tools/check.sh --full", text)
        self.assertNotIn("stackfish-db", text)


if __name__ == "__main__":
    unittest.main()
