"""deploy/pin-actions.py: pins by the commit, dereferences annotated tags, and refuses what is not a version. The "remote" is a real git repository on disk
(`git ls-remote` reads it as it reads GitHub); nothing is imitated. Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
ROOT = DEPLOY.parent
spec = importlib.util.spec_from_file_location("pin_actions", DEPLOY / "pin-actions.py")
pin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pin)

ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, env=ENV, capture_output=True, text=True, check=True).stdout.strip()


class Remotes:
    """owner/tool.git with: v1.0.0 (lightweight), v1.2.0 and v1.2.3 (annotated), v1 (a moving lightweight tag at v1.2.3), v2.0.0, v2.1.0-rc1, and a branch `stable`."""

    def __init__(self):
        self.dir = tempfile.TemporaryDirectory()
        self.base = pathlib.Path(self.dir.name)
        work = self.base / "work"
        work.mkdir()
        git(work, "init", "-q", "-b", "main")
        self.commits = {}
        for name in ["1.0.0", "1.2.0", "1.2.3", "2.0.0", "2.1.0-rc1"]:
            (work / "f").write_text(name)
            git(work, "add", "f")
            git(work, "commit", "-q", "-m", name)
            self.commits[name] = git(work, "rev-parse", "HEAD")
            if name in ("1.2.0", "1.2.3"):
                git(work, "tag", "-a", f"v{name}", "-m", name)
            else:
                git(work, "tag", f"v{name}")
            if name == "1.2.3":
                git(work, "tag", "v1")
        git(work, "branch", "stable")
        (self.base / "owner").mkdir()
        git(self.base, "clone", "-q", "--bare", str(work), str(self.base / "owner" / "tool.git"))
        pin.REMOTE = f"file://{self.base}/{{repo}}.git"

    def close(self):
        self.dir.cleanup()


def wf(*lines):
    return "\n".join(["jobs:", "  a:", "    steps:", *[f"      - uses: {u}" for u in lines]]) + "\n"


class Pin(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.remote = Remotes()

    @classmethod
    def tearDownClass(cls):
        cls.remote.close()

    def rewrite(self, *uses):
        return pin.rewrite(wf(*uses), pin.Tags())

    def test_a_major_becomes_the_commit_of_its_highest_release(self):
        text, done, failed = self.rewrite("owner/tool@v1")
        self.assertEqual(failed, [])
        self.assertIn(f"owner/tool@{self.remote.commits['1.2.3']} # v1.2.3", text)

    def test_an_annotated_tag_is_pinned_by_its_commit_not_by_the_tag_object(self):
        tag_object = git(self.remote.base / "owner" / "tool.git", "rev-parse", "v1.2.3")
        commit = git(self.remote.base / "owner" / "tool.git", "rev-parse", "v1.2.3^{commit}")
        self.assertNotEqual(tag_object, commit, "the fixture has no annotated tag")
        text, _, failed = self.rewrite("owner/tool@v1.2.3")
        self.assertEqual(failed, [])
        self.assertIn(f"owner/tool@{commit} # v1.2.3", text)
        self.assertNotIn(tag_object, text)

    def test_a_minor_takes_the_highest_patch_and_an_exact_tag_that_tag(self):
        text, _, _ = self.rewrite("owner/tool@v1.2", "owner/tool@v1.0.0", "owner/tool@v2")
        self.assertIn(f"{self.remote.commits['1.2.3']} # v1.2.3", text)
        self.assertIn(f"{self.remote.commits['1.0.0']} # v1.0.0", text)
        self.assertIn(f"{self.remote.commits['2.0.0']} # v2.0.0", text)  # not the release candidate v2.1.0-rc1

    def test_a_branch_is_refused_and_left_as_it_is(self):
        text, done, failed = self.rewrite("owner/tool@stable")
        self.assertEqual(done, [])
        self.assertEqual(len(failed), 1)
        self.assertIn("not a version tag", failed[0])
        self.assertIn("owner/tool@stable", text)

    def test_a_version_that_does_not_exist_is_refused(self):
        _, _, failed = self.rewrite("owner/tool@v9")
        self.assertEqual(len(failed), 1)
        self.assertIn("no tag", failed[0])

    def test_a_repository_that_does_not_exist_stops_the_run(self):
        with self.assertRaises(SystemExit) as e:
            self.rewrite("nobody/nothing@v1")
        self.assertIn("nobody/nothing", str(e.exception))

    def test_a_directory_of_an_action_pins_its_repository(self):
        text, _, failed = self.rewrite("owner/tool/sub/dir@v1")
        self.assertEqual(failed, [])
        self.assertIn(f"owner/tool/sub/dir@{self.remote.commits['1.2.3']} # v1.2.3", text)

    def test_what_is_pinned_or_local_is_left_alone(self):
        before = wf(f"owner/tool@{self.remote.commits['1.2.3']} # v1.2.3", "./.github/actions/x", "docker://alpine@sha256:" + "a" * 64)
        text, done, failed = pin.rewrite(before, pin.Tags())
        self.assertEqual((text, done, failed), (before, [], []))

    def test_a_pin_without_its_version_gets_the_comment(self):
        text, done, _ = self.rewrite(f"owner/tool@{self.remote.commits['1.2.3']}")
        self.assertIn(f"owner/tool@{self.remote.commits['1.2.3']} # v1.2.3", text)
        self.assertEqual(len(done), 1)

    def test_the_indentation_the_dash_and_the_rest_of_the_file_are_kept(self):
        before = "a:\n  b:\n    - name: x\n      uses: owner/tool@v1\n      with:\n        k: v\n    - uses: owner/tool@v1.0.0   # old\n"
        text, _, _ = pin.rewrite(before, pin.Tags())
        lines = text.split("\n")
        self.assertEqual(lines[3], f"      uses: owner/tool@{self.remote.commits['1.2.3']} # v1.2.3")
        self.assertEqual(lines[4:6], ["      with:", "        k: v"])
        self.assertEqual(lines[6], f"    - uses: owner/tool@{self.remote.commits['1.0.0']} # v1.0.0")


class Check(unittest.TestCase):
    def test_what_is_pinned_passes(self):
        sha = "a" * 40
        self.assertEqual(pin.problems(wf(f"actions/checkout@{sha} # v7.0.1", "./.github/actions/x", "docker://alpine@sha256:" + "b" * 64)), [])

    def test_each_way_of_not_being_pinned_is_named_with_its_line(self):
        sha = "a" * 40
        found = pin.problems(wf("actions/checkout@v7", f"actions/checkout@{sha}", f"actions/checkout@{sha} # latest", "docker://alpine:3.20", 'actions/checkout@v7 # v7.0.1'))
        self.assertEqual([n for n, _ in found], [4, 5, 6, 7, 8])
        self.assertIn("tag or a branch", found[0][1])
        self.assertIn("version is missing", found[1][1])
        self.assertIn("version is missing", found[2][1])
        self.assertIn("digest", found[3][1])

    def test_a_quoted_or_computed_uses_is_unreadable_and_therefore_refused(self):
        found = pin.problems("steps:\n  - uses: 'actions/checkout@v7'\n  - uses: ${{ matrix.action }}\n")
        self.assertEqual(len(found), 2)
        self.assertIn("cannot be read", found[0][1])

    def test_the_script_exits_one_on_a_tree_with_a_tag_and_zero_on_the_repository(self):
        with tempfile.TemporaryDirectory() as d:
            w = pathlib.Path(d) / ".github" / "workflows"
            w.mkdir(parents=True)
            (w / "ci.yml").write_text(wf("actions/checkout@v7"))
            bad = subprocess.run([sys.executable, str(DEPLOY / "pin-actions.py"), "--check", "--root", d], capture_output=True, text=True)
        self.assertEqual(bad.returncode, 1)
        self.assertIn("ci.yml:4", bad.stdout)
        ok = subprocess.run([sys.executable, str(DEPLOY / "pin-actions.py"), "--check"], capture_output=True, text=True)
        self.assertEqual(ok.returncode, 0, ok.stdout)


class Verify(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.remote = Remotes()

    @classmethod
    def tearDownClass(cls):
        cls.remote.close()

    def test_a_pin_that_is_not_the_commit_of_its_tag_is_found(self):
        good = f"owner/tool@{self.remote.commits['1.2.3']} # v1.2.3"
        wrong = f"owner/tool@{self.remote.commits['1.0.0']} # v1.2.3"
        missing = f"owner/tool@{self.remote.commits['1.0.0']} # v7.7.7"
        self.assertEqual(pin.verify(wf(good), pin.Tags()), [])
        found = pin.verify(wf(wrong, missing), pin.Tags())
        self.assertEqual([n for n, _ in found], [4, 5])
        self.assertIn("v1.2.3 is", found[0][1])
        self.assertIn("does not exist", found[1][1])

    def test_the_tag_object_of_an_annotated_tag_is_not_the_commit(self):
        tag_object = git(self.remote.base / "owner" / "tool.git", "rev-parse", "v1.2.3")
        found = pin.verify(wf(f"owner/tool@{tag_object} # v1.2.3"), pin.Tags())
        self.assertEqual(len(found), 1)


if __name__ == "__main__":
    unittest.main()
