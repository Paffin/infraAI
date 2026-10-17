"""deploy/ci-ok.py: the one verdict of ci.yml. The input is what GitHub gives a job, `toJSON(needs)`. Run: python3 -m unittest discover -s deploy/tests"""
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("ci_ok", DEPLOY / "ci-ok.py")
ci_ok = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_ok)

JOBS = ["docs", "backend", "msrv", "frontend", "security", "manifests", "live-llm", "iac", "build", "image-checks", "e2e", "e2e-report"]


def needs(code, **results):
    """What GitHub passes for a run: every job `success` unless said otherwise; live-llm is skipped outside a manual run."""
    out = {"scope": {"result": "success", "outputs": {"code": code}}}
    for job in JOBS:
        out[job] = {"result": "skipped" if job == "live-llm" else "success", "outputs": {}}
    for job, result in results.items():
        out[job.replace("_", "-")] = {"result": result, "outputs": {}}
    return out


class Verdict(unittest.TestCase):
    def test_everything_green_is_green(self):
        ok, lines, bad = ci_ok.verdict(needs("true"))
        self.assertTrue(ok, bad)
        self.assertEqual(bad, [])

    def test_a_failed_or_cancelled_job_is_red(self):
        for result in ["failure", "cancelled"]:
            with self.subTest(result):
                ok, _, bad = ci_ok.verdict(needs("true", e2e=result))
                self.assertFalse(ok)
                self.assertEqual(bad, [f"e2e: {result}"])

    def test_a_skipped_job_is_red_when_the_change_is_code(self):
        # the job that depended on a failed one is skipped; a required check that turned green then would guard nothing
        ok, _, bad = ci_ok.verdict(needs("true", image_checks="skipped"))
        self.assertFalse(ok)
        self.assertIn("image-checks was skipped", bad[0])

    def test_a_documentation_only_change_may_skip_the_heavy_jobs(self):
        skipped = {j.replace("-", "_"): "skipped" for j in JOBS if j not in ("docs",)}
        ok, _, bad = ci_ok.verdict(needs("false", **skipped))
        self.assertTrue(ok, bad)

    def test_but_docs_and_scope_are_never_skipped(self):
        ok, _, bad = ci_ok.verdict(needs("false", docs="skipped"))
        self.assertFalse(ok)
        self.assertIn("docs is never skipped", bad[0])
        n = needs("false")
        n["scope"]["result"] = "skipped"
        self.assertFalse(ci_ok.verdict(n)[0])

    def test_docs_failing_is_red_also_for_documentation(self):
        skipped = {j.replace("-", "_"): "skipped" for j in JOBS if j not in ("docs",)}
        self.assertFalse(ci_ok.verdict(needs("false", docs="failure", **skipped))[0])

    def test_a_failed_scope_is_red_and_everything_after_it_is_skipped(self):
        n = needs("true", **{j.replace("-", "_"): "skipped" for j in JOBS if j != "docs"})
        n["scope"] = {"result": "failure", "outputs": {}}
        ok, _, bad = ci_ok.verdict(n)
        self.assertFalse(ok)
        self.assertIn("scope: failure", bad)

    def test_a_missing_scope_is_red(self):
        n = needs("true")
        del n["scope"]
        ok, _, bad = ci_ok.verdict(n)
        self.assertFalse(ok)
        self.assertIn("scope is not among", bad[-1])

    def test_a_manual_run_that_runs_live_llm_needs_it_green(self):
        self.assertTrue(ci_ok.verdict(needs("true", live_llm="success"))[0])
        self.assertFalse(ci_ok.verdict(needs("true", live_llm="failure"))[0])

    def test_an_unknown_result_is_red(self):
        self.assertFalse(ci_ok.verdict(needs("true", backend="neutral"))[0])


class TheScript(unittest.TestCase):
    def run_with(self, payload):
        return subprocess.run([sys.executable, str(DEPLOY / "ci-ok.py")], env={**os.environ, "NEEDS": json.dumps(payload)}, capture_output=True, text=True)

    def test_exit_codes_and_annotation(self):
        green = self.run_with(needs("true"))
        self.assertEqual(green.returncode, 0, green.stdout)
        red = self.run_with(needs("true", backend="failure"))
        self.assertEqual(red.returncode, 1)
        self.assertIn("::error title=ci-ok::backend: failure", red.stdout)

    def test_no_input_is_red(self):
        p = subprocess.run([sys.executable, str(DEPLOY / "ci-ok.py")], env={k: v for k, v in os.environ.items() if k != "NEEDS"}, capture_output=True, text=True)
        self.assertEqual(p.returncode, 1)


if __name__ == "__main__":
    unittest.main()
