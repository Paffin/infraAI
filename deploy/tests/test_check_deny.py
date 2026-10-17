"""deploy/check-deny.py: an ignored advisory has a reason and a review date that has not passed. Run: python3 -m unittest discover -s deploy/tests"""
import datetime
import importlib.util
import pathlib
import subprocess
import sys
import unittest

DEPLOY = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("check_deny", DEPLOY / "check-deny.py")
check_deny = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_deny)

TODAY = datetime.date(2026, 10, 17)


def deny(ignore):
    return f"[advisories]\nignore = {ignore}\n"


class IgnoredAdvisories(unittest.TestCase):
    def test_nothing_ignored_is_fine(self):
        self.assertEqual(check_deny.problems(deny("[]"), TODAY), ([], 0))
        self.assertEqual(check_deny.problems("[bans]\nmultiple-versions = 'warn'\n", TODAY), ([], 0))

    def test_an_entry_with_a_reason_and_a_future_date_is_fine(self):
        text = deny('[{ id = "RUSTSEC-2099-0001", reason = "the feature that has the flaw is off; review-by 2027-01-31" }]')
        self.assertEqual(check_deny.problems(text, TODAY), ([], 1))
        # the date is the last day it holds: today is not yet past
        self.assertEqual(check_deny.problems(deny('[{ id = "RUSTSEC-2099-0001", reason = "x review-by 2026-10-17" }]'), TODAY)[0], [])

    def test_a_bare_string_has_no_reason(self):
        found, count = check_deny.problems(deny('["RUSTSEC-2099-0001"]'), TODAY)
        self.assertEqual(count, 1)
        self.assertEqual(len(found), 1)
        self.assertIn("without a reason", found[0])

    def test_a_reason_without_a_date_or_an_empty_one_is_refused(self):
        for reason in ["", "   ", "it is not reachable", "review soon", "review-by someday"]:
            found, _ = check_deny.problems(deny('[{ id = "RUSTSEC-2099-0001", reason = "%s" }]' % reason), TODAY)
            self.assertEqual(len(found), 1, reason)
            self.assertIn("RUSTSEC-2099-0001", found[0])

    def test_a_date_that_has_passed_or_is_not_a_date_is_refused(self):
        found, _ = check_deny.problems(deny('[{ id = "RUSTSEC-2099-0001", reason = "x review-by 2026-10-16" }]'), TODAY)
        self.assertEqual(len(found), 1)
        self.assertIn("has passed", found[0])
        found, _ = check_deny.problems(deny('[{ id = "RUSTSEC-2099-0001", reason = "x review-by 2026-13-45" }]'), TODAY)
        self.assertIn("is not a date", found[0])

    def test_a_yanked_crate_entry_is_judged_the_same_way(self):
        found, _ = check_deny.problems(deny('[{ crate = "left-pad@0.1.1", reason = "x" }]'), TODAY)
        self.assertEqual(len(found), 1)
        self.assertIn("left-pad@0.1.1", found[0])

    def test_every_bad_entry_is_listed_not_only_the_first(self):
        text = deny('["RUSTSEC-2099-0001", { id = "RUSTSEC-2099-0002", reason = "r" }, { id = "RUSTSEC-2099-0003", reason = "r review-by 2027-01-01" }]')
        found, count = check_deny.problems(text, TODAY)
        self.assertEqual((len(found), count), (2, 3))

    def test_the_real_file_passes_and_the_script_exits_with_the_verdict(self):
        run = lambda *a: subprocess.run([sys.executable, str(DEPLOY / "check-deny.py"), *a], capture_output=True, text=True)
        ok = run()
        self.assertEqual(ok.returncode, 0, ok.stdout + ok.stderr)
        # a file with a stale ignore fails the same program
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            bad = pathlib.Path(d) / "deny.toml"
            bad.write_text(deny('[{ id = "RUSTSEC-2099-0001", reason = "x review-by 2020-01-01" }]'))
            failed = run(str(bad))
            self.assertEqual(failed.returncode, 1, failed.stdout)
            self.assertIn("has passed", failed.stdout)
            # and with the clock set back, the same file is fine
            self.assertEqual(run(str(bad), "--today", "2019-12-31").returncode, 0)


if __name__ == "__main__":
    unittest.main()
