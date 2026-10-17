"""deploy/check-dependabot.py: every place Dependabot can watch has an entry. The trees are built in a temporary directory, the check runs on them as on the repository.
Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import textwrap
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
ROOT = DEPLOY.parent
spec = importlib.util.spec_from_file_location("check_dependabot", DEPLOY / "check-dependabot.py")
dep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dep)

CONFIG = """
version: 2
updates:
  - package-ecosystem: cargo
    directory: /backend
    schedule: { interval: weekly }
  - package-ecosystem: npm
    directory: /frontend
    schedule: { interval: weekly }
  - package-ecosystem: github-actions
    directories: ["/", "/.github/actions/*"]
    schedule: { interval: weekly }
  - package-ecosystem: docker
    directories: ["/", "/deploy/iac-check"]
    schedule: { interval: weekly }
  - package-ecosystem: docker-compose
    directories: ["/", "/deploy"]
    schedule: { interval: weekly }
  - package-ecosystem: pip
    directory: /deploy
    schedule: { interval: weekly }
"""

TREE = {
    "backend/Cargo.toml": '[workspace]\nmembers = ["crates/*"]\n',
    "backend/Cargo.lock": "",
    "backend/crates/engine/Cargo.toml": '[package]\nname = "engine"\n',
    "backend/crates/server/Cargo.toml": '[package]\nname = "server"\n',
    "frontend/package.json": "{}",
    "frontend/package-lock.json": "{}",
    "Dockerfile": "FROM x\n",
    "docker-compose.yml": "services: {}\n",
    "docker-compose.test.yml": "services: {}\n",
    "deploy/compose.try.yml": "services: {}\n",
    "deploy/iac-check/Dockerfile": "FROM x\n",
    "deploy/requirements-ci.txt": "pyyaml==6.0.2\n",
    ".github/workflows/ci.yml": "name: CI\n",
    ".github/actions/tool/action.yml": "name: Tool\n",
}


class Tree:
    def __init__(self, extra=None, config=CONFIG, without=()):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        for name, text in {**TREE, **(extra or {})}.items():
            if name in without:
                continue
            f = self.root / name
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text(text)
        if config is not None:
            (self.root / ".github" / "dependabot.yml").write_text(textwrap.dedent(config))

    def check(self):
        return dep.check(self.root)[0]

    def close(self):
        self.dir.cleanup()


class Covered(unittest.TestCase):
    def test_a_tree_where_everything_has_an_entry_passes(self):
        t = Tree()
        try:
            problems, places, entries = dep.check(t.root)
            self.assertEqual(problems, [])
            self.assertEqual(entries, 6)
            # backend (cargo), frontend (npm), / and the action (github-actions), / and deploy/iac-check (docker), / and deploy (compose), deploy (pip)
            self.assertEqual(places, 9)
        finally:
            t.close()

    def test_the_members_of_a_workspace_are_the_roots_not_places_of_their_own(self):
        t = Tree()
        try:
            found = dep.needs(t.root)
            self.assertIn(("cargo", "/backend"), found)
            self.assertNotIn(("cargo", "/backend/crates/engine"), found)
            self.assertNotIn(("cargo", "/backend/crates/server"), found)
        finally:
            t.close()

    def test_a_crate_on_its_own_is_a_place(self):
        t = Tree({"tools/helper/Cargo.toml": '[package]\nname = "helper"\n'})
        try:
            self.assertTrue(any("cargo /tools/helper" in p for p in t.check()))
        finally:
            t.close()


class Uncovered(unittest.TestCase):
    def assert_one(self, tree, *words):
        try:
            problems = tree.check()
            self.assertEqual(len(problems), 1, problems)
            for w in words:
                self.assertIn(w, problems[0])
        finally:
            tree.close()

    def test_a_dockerfile_in_a_new_directory(self):
        self.assert_one(Tree({"tools/image/Dockerfile": "FROM x\n"}), "docker /tools/image", "Dockerfile", "package-ecosystem: docker")

    def test_a_dockerfile_with_a_suffix_and_a_named_one(self):
        self.assert_one(Tree({"a/Dockerfile.prod": "FROM x\n"}), "docker /a")
        self.assert_one(Tree({"b/app.Dockerfile": "FROM x\n"}), "docker /b")

    def test_a_requirements_file_in_a_new_directory(self):
        self.assert_one(Tree({"tools/requirements.txt": "x==1\n"}), "pip /tools", "requirements.txt")

    def test_a_package_json_without_a_lock_file_and_with_one(self):
        self.assert_one(Tree({"site/package.json": "{}"}), "npm /site", "package.json")
        self.assert_one(Tree({"site/package.json": "{}", "site/package-lock.json": "{}"}), "npm /site", "package-lock.json")

    def test_a_compose_file_in_a_new_directory(self):
        self.assert_one(Tree({"ops/compose.prod.yaml": "services: {}\n"}), "docker-compose /ops")

    def test_a_new_composite_action(self):
        self.assert_one(Tree({".github/actions/other/action.yml": "name: O\n"}, config=CONFIG.replace('"/.github/actions/*"', '"/.github/actions/tool"')), "github-actions /.github/actions/other")

    def test_workflows_without_a_github_actions_entry(self):
        config = CONFIG.replace("""  - package-ecosystem: github-actions
    directories: ["/", "/.github/actions/*"]
    schedule: { interval: weekly }
""", "")
        t = Tree(config=config)
        try:
            problems = t.check()
            self.assertEqual(len(problems), 2, problems)  # the workflows and the composite action
            self.assertTrue(all("github-actions" in p for p in problems))
        finally:
            t.close()

    def test_an_entry_of_another_ecosystem_for_the_same_directory_does_not_cover_it(self):
        # /deploy has a pip entry and a compose entry; remove the compose one and compose.try.yml is uncovered although /deploy is "watched"
        config = CONFIG.replace('directories: ["/", "/deploy"]', 'directories: ["/"]')
        self.assert_one(Tree(config=config), "docker-compose /deploy", "compose.try.yml")

    def test_no_config_at_all(self):
        t = Tree(config=None)
        try:
            problems = t.check()
            self.assertEqual(len(problems), 1)
            self.assertIn("does not exist", problems[0])
        finally:
            t.close()


class EntriesThatWatchNothing(unittest.TestCase):
    def test_an_entry_for_a_directory_that_does_not_exist_is_a_mistake(self):
        t = Tree(config=CONFIG.replace("directory: /deploy\n", "directory: /deploi\n"))
        try:
            problems = t.check()
            self.assertTrue(any("pip /deploi" in p and "does not exist" in p for p in problems), problems)
        finally:
            t.close()


class WhatIsLeftOut(unittest.TestCase):
    def test_dependencies_dirs_and_test_fixtures_are_not_looked_into(self):
        t = Tree({
            "frontend/node_modules/x/package.json": "{}",
            "backend/target/debug/Cargo.lock": "",
            "frontend/dist/Dockerfile": "FROM x\n",
            "deploy/tests/fixtures/dependabot/uncovered/Dockerfile": "FROM x\n",
            ".git/hooks/requirements.txt": "x\n",
        })
        try:
            self.assertEqual(t.check(), [])
        finally:
            t.close()


class TheScript(unittest.TestCase):
    def test_exit_codes_on_a_good_and_a_bad_tree(self):
        run = lambda root: subprocess.run([sys.executable, str(DEPLOY / "check-dependabot.py"), str(root)], capture_output=True, text=True)
        good, bad = Tree(), Tree({"tools/image/Dockerfile": "FROM x\n"})
        try:
            ok = run(good.root)
            self.assertEqual(ok.returncode, 0, ok.stdout)
            no = run(bad.root)
            self.assertEqual(no.returncode, 1)
            self.assertIn("docker /tools/image", no.stdout)
        finally:
            good.close()
            bad.close()


class TheRealRepository(unittest.TestCase):
    def test_every_place_has_an_entry(self):
        problems, places, entries = dep.check(ROOT)
        self.assertEqual(problems, [])
        self.assertGreaterEqual(places, 9)

    def test_cargo_and_npm_wait_a_few_days_for_a_new_release(self):
        import yaml

        config = yaml.safe_load((ROOT / ".github" / "dependabot.yml").read_text())
        for entry in config["updates"]:
            cooldown = entry.get("cooldown") or {}
            self.assertGreaterEqual(cooldown.get("default-days", 0), 5, entry["package-ecosystem"])
            if entry["package-ecosystem"] in ("cargo", "npm"):
                self.assertGreaterEqual(cooldown.get("semver-major-days", 0), 14, entry["package-ecosystem"])


if __name__ == "__main__":
    unittest.main()
