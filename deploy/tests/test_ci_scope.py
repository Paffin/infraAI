"""deploy/ci-scope.py: only a change that is documentation from end to end skips the heavy jobs. The repositories here are real (git init, commits, branches); nothing
about git is imitated. Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
ROOT = DEPLOY.parent
spec = importlib.util.spec_from_file_location("ci_scope", DEPLOY / "ci-scope.py")
scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scope)

GIT_ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull,
}


class Repo:
    """A throw-away repository whose `main` holds the files in `files`; `change` makes one more commit on the branch `pr`."""

    def __init__(self, files):
        self._dir = tempfile.TemporaryDirectory()
        self.path = pathlib.Path(self._dir.name)
        self.git("init", "-q", "-b", "main")
        self.write(files)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("checkout", "-q", "-b", "pr")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, text=True, check=True).stdout

    def write(self, files):
        for name, text in files.items():
            p = self.path / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)

    def change(self, write=None, delete=(), move=()):
        self.write(write or {})
        for name in delete:
            self.git("rm", "-q", name)
        for old, new in move:
            (self.path / new).parent.mkdir(parents=True, exist_ok=True)
            self.git("mv", old, new)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")

    def decide(self, event="pull_request", base=None):
        return scope.decide(event, self.base if base is None else base, "HEAD", cwd=self.path)

    def close(self):
        self._dir.cleanup()


BASE_FILES = {
    "README.md": "a\n", "CHANGELOG.md": "a\n", "LICENSE": "a\n", "NOTICE": "a\n", "THIRD_PARTY_LICENSES.md": "a\n",
    "docs/DEVELOPMENT.md": "a\n", "docs/API.md": "a\n", "docs/images/logo.svg": "<svg/>\n",
    "backend/crates/engine/src/lib.rs": "// a\n", "frontend/README.md": "a\n", "frontend/src/main.ts": "// a\n",
    ".github/workflows/ci.yml": "name: CI\n", ".github/ISSUE_TEMPLATE/bug.md": "a\n", ".github/CODEOWNERS": "* @a\n",
}


class OnRealRepositories(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo = Repo(BASE_FILES)

    @classmethod
    def tearDownClass(cls):
        cls.repo.close()

    def setUp(self):
        # every test starts from the same branch: `pr` is reset to the base
        self.repo.git("reset", "-q", "--hard", self.repo.base)

    def test_a_change_of_documentation_alone_skips_the_heavy_jobs(self):
        self.repo.change({"docs/DEVELOPMENT.md": "b\n", "README.md": "b\n", "CHANGELOG.md": "b\n", "docs/images/logo.svg": "<svg>b</svg>\n", "docs/new-page.md": "new\n"})
        code, why = self.repo.decide()
        self.assertFalse(code, why)
        self.assertIn("only documentation", why)

    def test_one_source_file_among_the_documentation_runs_everything(self):
        self.repo.change({"docs/DEVELOPMENT.md": "b\n", "backend/crates/engine/src/lib.rs": "// b\n"})
        code, why = self.repo.decide()
        self.assertTrue(code)
        self.assertIn("backend/crates/engine/src/lib.rs", why)

    def test_what_the_image_carries_is_not_documentation(self):
        for name in ["THIRD_PARTY_LICENSES.md", "LICENSE", "NOTICE", "frontend/README.md"]:
            with self.subTest(name):
                self.repo.git("reset", "-q", "--hard", self.repo.base)
                self.repo.change({name: "changed\n"})
                self.assertTrue(self.repo.decide()[0], name)

    def test_a_page_a_test_reads_is_not_documentation(self):
        self.repo.change({"docs/API.md": "an error code lost its row\n"})
        self.assertTrue(self.repo.decide()[0])

    def test_a_new_directory_and_the_workflows_are_code(self):
        for name in ["examples/foo.yaml", "action.yml", ".github/workflows/ci.yml", "deploy/smoke.sh", "Makefile", ".gitignore", "docs.txt"]:
            with self.subTest(name):
                self.repo.git("reset", "-q", "--hard", self.repo.base)
                self.repo.change({name: "changed\n"})
                self.assertTrue(self.repo.decide()[0], name)

    def test_templates_and_owners_are_documentation(self):
        self.repo.change({".github/ISSUE_TEMPLATE/bug.md": "b\n", ".github/ISSUE_TEMPLATE/idea.md": "new\n", ".github/CODEOWNERS": "* @b\n", ".github/PULL_REQUEST_TEMPLATE.md": "new\n"})
        self.assertFalse(self.repo.decide()[0])

    def test_moving_a_source_file_into_docs_is_a_change_of_the_source(self):
        # `git diff` alone would list only the new path, docs/lib.md, and the deletion of the source would go unseen
        self.repo.change(move=[("backend/crates/engine/src/lib.rs", "docs/lib.md")])
        code, why = self.repo.decide()
        self.assertTrue(code)
        self.assertIn("backend/crates/engine/src/lib.rs", why)

    def test_deleting_a_source_file_is_code(self):
        self.repo.change(delete=["frontend/src/main.ts"])
        self.assertTrue(self.repo.decide()[0])

    def test_a_pull_request_that_changes_nothing_is_not_read_as_documentation(self):
        code, why = self.repo.decide()
        self.assertTrue(code)
        self.assertIn("empty", why)

    def test_main_and_manual_runs_run_everything_even_for_documentation(self):
        self.repo.change({"docs/DEVELOPMENT.md": "b\n"})
        self.assertFalse(self.repo.decide("pull_request")[0])
        for event in ["push", "workflow_dispatch", "schedule", ""]:
            with self.subTest(event):
                self.assertTrue(self.repo.decide(event)[0])

    def test_an_unknown_or_missing_base_runs_everything(self):
        self.repo.change({"docs/DEVELOPMENT.md": "b\n"})
        self.assertTrue(self.repo.decide(base="")[0])
        code, why = self.repo.decide(base="0" * 40)
        self.assertTrue(code)
        self.assertIn("git could not list", why)

    def test_a_directory_that_is_no_repository_runs_everything(self):
        with tempfile.TemporaryDirectory() as d:
            code, why = scope.decide("pull_request", "a" * 40, "HEAD", cwd=d)
        self.assertTrue(code)
        self.assertIn("git could not list", why)


class TheScript(unittest.TestCase):
    def run_script(self, repo, *args):
        out = repo.path / "github_output"
        summary = repo.path / "github_summary"
        # a workflow gives every step an empty file; the script appends to it
        out.unlink(missing_ok=True)
        summary.unlink(missing_ok=True)
        env = {**GIT_ENV, "GITHUB_OUTPUT": str(out), "GITHUB_STEP_SUMMARY": str(summary)}
        p = subprocess.run([sys.executable, str(DEPLOY / "ci-scope.py"), *args], cwd=repo.path, env=env, capture_output=True, text=True)
        return p, out.read_text() if out.exists() else "", summary.read_text() if summary.exists() else ""

    def test_it_writes_the_verdict_where_a_workflow_reads_it(self):
        repo = Repo(BASE_FILES)
        try:
            repo.change({"docs/DEVELOPMENT.md": "b\n"})
            p, out, summary = self.run_script(repo, "--event", "pull_request", "--base", repo.base)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertEqual(out, "code=false\n")
            self.assertIn("documentation only", summary)
            p, out, summary = self.run_script(repo, "--event", "push", "--base", repo.base)
            self.assertEqual(out, "code=true\n")
            self.assertIn("everything runs", summary)
        finally:
            repo.close()


class ThePagesTheTestsRead(unittest.TestCase):
    """docs/API.md is code because two tests of the server read it. The next page a test starts reading must not be skipped by accident: this fails first."""

    READS = re.compile(r"read_to_string|include_str!|readFileSync|read_text|fs\.read")
    PAGE = re.compile(r"docs/([A-Za-z0-9_./-]+\.md)")

    def sources(self):
        for base, suffixes in [("backend/crates", {".rs"}), ("frontend/tests", {".ts"}), ("frontend/e2e", {".ts"}), ("frontend/src", {".ts", ".vue"})]:
            for p in (ROOT / base).rglob("*"):
                if p.suffix in suffixes and "node_modules" not in p.parts and "target" not in p.parts and p.is_file():
                    yield p

    def test_every_page_of_docs_that_a_test_reads_is_code_for_the_scope(self):
        found = {}
        for p in self.sources():
            for n, line in enumerate(p.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                if self.READS.search(line):
                    for page in self.PAGE.findall(line):
                        found.setdefault("docs/" + page, f"{p.relative_to(ROOT)}:{n}")
        self.assertIn("docs/API.md", found, "the scan no longer sees the two tests that read docs/API.md: it has stopped looking")
        for page, where in found.items():
            self.assertTrue((ROOT / page).exists(), f"{where} reads {page}, which does not exist")
            self.assertFalse(scope.is_docs(page), f"{where} reads {page}: add it to READ_BY_TESTS in deploy/ci-scope.py, or a change to it alone would skip the tests that read it")


class Classification(unittest.TestCase):
    def test_the_table(self):
        docs = ["README.md", "README.en.md", "CHANGELOG.md", "CONTRIBUTING.md", "docs/PITFALLS.md", "docs/adr/0001-x.md", "docs/images/banner.svg", ".github/dependabot.yml", ".github/CODEOWNERS"]
        code = ["THIRD_PARTY_LICENSES.md", "LICENSE", "NOTICE", "docs/API.md", "Dockerfile", "Makefile", "frontend/README.md", "backend/Cargo.lock", ".github/workflows/ci.yml", ".github/actions/rust-toolchain/action.yml", "docs", "docsx/a.md", "deploy/README.md"]
        for p in docs:
            self.assertTrue(scope.is_docs(p), p)
        for p in code:
            self.assertFalse(scope.is_docs(p), p)

    def test_an_empty_list_and_blank_lines(self):
        self.assertTrue(scope.classify([])[0])
        self.assertTrue(scope.classify(["", ""])[0])
        self.assertFalse(scope.classify(["", "README.md"])[0])


if __name__ == "__main__":
    unittest.main()
