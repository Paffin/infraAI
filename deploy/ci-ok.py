#!/usr/bin/env python3
"""The one verdict of ci.yml: `ci-ok`. Mark THIS job as the required check of the repository, not the jobs one by one.

    NEEDS='<toJSON(needs)>' python3 deploy/ci-ok.py

`needs` is what GitHub gives a job about the jobs it waits for: {"backend": {"result": "success", "outputs": {}}, "scope": {"result": "success", "outputs": {"code": "true"}}, …}.

A job is accepted when it succeeded. `skipped` is accepted only where skipping is the plan: when `scope` found that nothing but documentation changed
(`code=false`: the heavy jobs are skipped on purpose; `scope` and `docs` are never skipped), and for `live-llm`, which runs only on request. A skipped job in
any other case is a red `ci-ok`: it means something the job depended on failed or the workflow's own conditions are wrong, and a required check that turns green
on a skipped job guards nothing (that is the way a required job that is skipped is reported as passing in GitHub). `failure`, `cancelled` and anything else are red.
"""
import json
import os
import sys

NEVER_SKIPPED = {"scope", "docs"}
SKIPPED_BY_DESIGN = {"live-llm"}


def verdict(needs):
    """(ok, one line per job, the reason of each job that is not accepted)."""
    code = ((needs.get("scope") or {}).get("outputs") or {}).get("code")
    docs_only = code == "false"
    bad, lines = [], []
    for job in sorted(needs):
        result = (needs[job] or {}).get("result", "?")
        if result == "success":
            ok = True
        elif result == "skipped":
            ok = job in SKIPPED_BY_DESIGN or (docs_only and job not in NEVER_SKIPPED)
        else:
            ok = False
        lines.append(f"{'ok  ' if ok else 'FAIL'} {job:<14} {result}")
        if not ok:
            if result == "skipped":
                bad.append(f"{job} was skipped, but this change is not documentation-only (scope: code={code or 'unknown'})" if not docs_only else f"{job} is never skipped")
            else:
                bad.append(f"{job}: {result}")
    if "scope" not in needs:
        bad.append("scope is not among the jobs ci-ok waits for")
    return not bad, lines, bad


def main():
    needs = json.loads(os.environ.get("NEEDS") or "{}")
    ok, lines, bad = verdict(needs)
    print("\n".join(lines))
    scope = ((needs.get("scope") or {}).get("outputs") or {}).get("code")
    print(f"\nscope: code={scope}" + (" (documentation only: the heavy jobs were skipped on purpose)" if scope == "false" else ""))
    for b in bad:
        print(f"::error title=ci-ok::{b}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
