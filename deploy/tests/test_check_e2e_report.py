"""deploy/check-e2e-report.py: the browser tests that were listed are the browser tests that ran. The fixtures are the real output of `playwright test --list --reporter=json`
and `playwright merge-reports --reporter=json` (a small project of ten tests: passing, failing, skipped, passing on a retry), reduced to the fields the check reads.
Run: python3 -m unittest discover -s deploy/tests"""
import copy
import importlib.util
import json
import pathlib
import subprocess
import sys
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
FIX = DEPLOY / "tests" / "fixtures" / "playwright"
spec = importlib.util.spec_from_file_location("check_e2e_report", DEPLOY / "check-e2e-report.py")
rep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rep)


def load(name):
    return json.loads((FIX / name).read_text())


def specs(doc):
    out = []
    for top in doc["suites"]:
        stack = [top]
        while stack:
            s = stack.pop()
            out += s.get("specs", [])
            stack += s.get("suites", [])
    return out


def drop(doc, title):
    """The same report without the spec of that title (a shard that never reported it)."""
    doc = copy.deepcopy(doc)
    stack = list(doc["suites"])
    while stack:
        s = stack.pop()
        s["specs"] = [x for x in s.get("specs", []) if x["title"] != title]
        stack += s.get("suites", [])
    return doc


class Verdict(unittest.TestCase):
    def test_a_suite_that_ran_whole_and_passed_is_fine(self):
        problems, warnings, summary = rep.verdict(load("list-ok.json"), load("merged-ok.json"))
        self.assertEqual((problems, warnings), ([], []))
        self.assertEqual(summary, "7 listed, 7 ran (7 passed, 0 flaky, 0 failed), 0 skipped")

    def test_a_test_that_no_shard_ran_is_named(self):
        merged = drop(load("merged-ok.json"), "six")
        problems, _, _ = rep.verdict(load("list-ok.json"), merged)
        self.assertEqual(len(problems), 2)
        self.assertIn("1 listed test(s) did not run", problems[0])
        self.assertIn("c.spec.ts > six", problems[0])

    def test_a_whole_shard_that_is_missing_is_counted(self):
        listed = load("list-ok.json")
        merged = load("merged-ok.json")
        merged["suites"] = [s for s in merged["suites"] if s["title"] != "c.spec.ts"]
        problems, _, summary = rep.verdict(listed, merged)
        self.assertIn("3 listed test(s) did not run", problems[0])
        self.assertTrue(summary.startswith("7 listed, 4 ran"))

    def test_a_test_that_two_shards_both_ran_is_named(self):
        merged = load("merged-ok.json")
        first = next(x for x in specs(merged) if x["title"] == "one")
        first["tests"].append(copy.deepcopy(first["tests"][0]))
        problems, _, _ = rep.verdict(load("list-ok.json"), merged)
        self.assertTrue(any("ran more than once" in p and "one" in p for p in problems), problems)

    def test_a_test_that_ran_but_is_not_listed_is_named(self):
        listed = drop(load("list-ok.json"), "seven")
        problems, _, _ = rep.verdict(listed, load("merged-ok.json"))
        self.assertTrue(any("ran that `--list` does not show" in p and "seven" in p for p in problems), problems)

    def test_a_skipped_test_is_refused(self):
        problems, _, _ = rep.verdict(load("list-mixed.json"), load("merged-mixed.json"))
        self.assertTrue(any("were skipped" in p and "nine is skipped" in p for p in problems), problems)

    def test_a_failed_test_is_named(self):
        problems, _, _ = rep.verdict(load("list-mixed.json"), load("merged-mixed.json"))
        self.assertTrue(any("failed" in p and "eight fails" in p for p in problems), problems)

    def test_a_test_that_passed_on_a_retry_is_a_warning_not_a_failure(self):
        problems, warnings, _ = rep.verdict(load("list-mixed.json"), load("merged-mixed.json"))
        self.assertTrue(any("ten is flaky" in w for w in warnings), warnings)
        self.assertFalse(any("ten is flaky" in p for p in problems), problems)

    def test_totals_that_disagree_with_the_tests_are_found(self):
        merged = load("merged-ok.json")
        merged["stats"]["expected"] = 9
        problems, _, _ = rep.verdict(load("list-ok.json"), merged)
        self.assertTrue(any("says 9 expected, the tests in it are 7" in p for p in problems), problems)

    def test_an_empty_report_misses_everything(self):
        listed = load("list-ok.json")
        problems, _, _ = rep.verdict(listed, {"suites": [], "stats": {}})
        self.assertIn("7 listed test(s) did not run", problems[0])


class TheScript(unittest.TestCase):
    def run_script(self, listed, merged):
        return subprocess.run([sys.executable, str(DEPLOY / "check-e2e-report.py"), str(FIX / listed), str(FIX / merged)], capture_output=True, text=True)

    def test_exit_codes_and_annotations(self):
        ok = self.run_script("list-ok.json", "merged-ok.json")
        self.assertEqual(ok.returncode, 0, ok.stdout)
        bad = self.run_script("list-mixed.json", "merged-mixed.json")
        self.assertEqual(bad.returncode, 1)
        self.assertIn("::error title=Browser tests::", bad.stdout)
        self.assertIn("::warning title=Flaky browser test::", bad.stdout)

    def test_the_list_of_the_repository_is_readable(self):
        # the real suite of the repository is listed by the same command CI runs; here only that the checker reads its shape (no browser is needed for --list)
        frontend = DEPLOY.parent / "frontend"
        if not (frontend / "node_modules" / "@playwright").is_dir():
            self.skipTest("frontend/node_modules is not installed")
        import os
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            out = pathlib.Path(d) / "list.json"
            p = subprocess.run(["npx", "playwright", "test", "--list", "--reporter=json"], cwd=frontend, env={**os.environ, "PLAYWRIGHT_JSON_OUTPUT_NAME": str(out)}, capture_output=True, text=True)
            self.assertEqual(p.returncode, 0, p.stderr[-400:])
            listed = json.loads(out.read_text())
        self.assertGreater(len(rep.tests_of(listed)), 100)
        # the list is also what a report with nothing in it is compared with
        problems, _, summary = rep.verdict(listed, {"suites": [], "stats": {}})
        self.assertTrue(problems)
        self.assertIn(" listed, 0 ran", summary)


if __name__ == "__main__":
    unittest.main()
