#!/usr/bin/env python3
"""Does a change touch anything but documentation? The first job of ci.yml asks, and the heavy jobs (build, browser tests, cargo, IaC) run only if it did.

    python3 deploy/ci-scope.py --event pull_request --base <sha of the base> [--head HEAD]

Prints and appends to $GITHUB_OUTPUT   code=true | code=false   (and a line to the job summary saying why).

The default is `code=true`. `code=false` needs a list of the changed files that is complete and in which EVERY path is documentation, so the ways to be wrong all
end in running too much, never too little: a push to main and a manual run always run everything (every commit of main is tested and may be published); an
unknown event, an unreachable base, a git error, an empty list - everything; a path this script has not been taught about - code. What documentation is, is a short list
and a tight one (DOCS_FILES, DOCS_PREFIXES and the top-level *.md files):

* a file the image or a test reads is NOT documentation, whatever its name: THIRD_PARTY_LICENSES.md, LICENSE and NOTICE travel in the image; docs/API.md is read by two
  tests of the server (every error code has a row in it; every heavy request is listed) - READ_BY_TESTS, and tests/test_ci_scope.py scans the sources of the tests for
  more of them, so a test that starts reading another page of docs/ fails there and not on main;
* the workflows themselves are code: a pull request that changes ci.yml must run what it changed;
* a rename is two changes (`git diff --no-renames`), so moving a source file into docs/ is seen as the deletion of the source.

`docs`, the cheap job that checks everything generated (tables, README numbers, licences, workflows), always runs.
"""
import argparse
import os
import pathlib
import subprocess
import sys

# The top-level files that are read by something (the image, a test) are not documentation even though they are text.
NOT_DOCS_AT_ROOT = {"THIRD_PARTY_LICENSES.md"}
# Pages of docs/ that a test reads (grep `read_to_string` in backend/crates/*/tests): changing one can fail `backend`.
READ_BY_TESTS = {"docs/API.md"}
DOCS_PREFIXES = ("docs/", ".github/ISSUE_TEMPLATE/")
DOCS_FILES = {".github/PULL_REQUEST_TEMPLATE.md", ".github/CODEOWNERS", ".github/dependabot.yml"}


def is_docs(path):
    """Is this one path (relative to the root of the repository, with `/`) documentation, i.e. may a change to it alone skip the heavy jobs?"""
    if path in READ_BY_TESTS or path in NOT_DOCS_AT_ROOT:
        return False
    if path in DOCS_FILES:
        return True
    if "/" not in path:
        return path.endswith(".md")
    return path.startswith(DOCS_PREFIXES)


def classify(paths):
    """(code, why): `code` is False only when the list is not empty and every path is documentation; `why` is a sentence for the job summary."""
    paths = [p for p in paths if p]
    if not paths:
        return True, "the list of changed files is empty, which is not read as 'nothing needs testing'"
    others = [p for p in paths if not is_docs(p)]
    if others:
        shown = ", ".join(f"`{p}`" for p in others[:5]) + (f" and {len(others) - 5} more" if len(others) > 5 else "")
        return True, f"{len(others)} of {len(paths)} changed files are not documentation: {shown}"
    return False, f"only documentation changed ({len(paths)} files): the build, the browser tests, cargo and the IaC check are skipped; `docs` and `ci-ok` run"


def changed_files(base, head="HEAD", cwd=None):
    """The paths that differ between `base` and `head`, renames as a deletion and an addition; raises on any git error (the caller runs everything)."""
    out = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", f"{base}...{head}", "--"],
        cwd=cwd, capture_output=True, check=True,
    ).stdout.decode("utf-8", "surrogateescape")
    return [p for p in out.split("\0") if p]


def decide(event, base, head="HEAD", cwd=None):
    """(code, why) for one run of the workflow."""
    if event != "pull_request":
        return True, f"a `{event or 'unknown'}` run always runs everything (every commit of main is tested and may be published)"
    if not base:
        return True, "the pull request gave no base commit"
    try:
        paths = changed_files(base, head, cwd)
    except (subprocess.CalledProcessError, OSError) as e:
        detail = (e.stderr.decode("utf-8", "replace").strip() if getattr(e, "stderr", None) else str(e))[:200]
        return True, f"git could not list the changed files ({detail}), so everything runs"
    return classify(paths)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", ""), help="github.event_name")
    ap.add_argument("--base", default="", help="the base commit of the pull request (github.event.pull_request.base.sha)")
    ap.add_argument("--head", default="HEAD")
    args = ap.parse_args(argv)
    code, why = decide(args.event, args.base, args.head)
    line = f"code={'true' if code else 'false'}"
    print(line)
    print(why)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
            f.write(line + "\n")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
            f.write(f"### Scope: {'everything runs' if code else 'documentation only'}\n\n{why}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
