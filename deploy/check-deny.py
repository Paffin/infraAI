#!/usr/bin/env python3
"""The ignored advisories of backend/deny.toml: each one says why and until when.

    python3 deploy/check-deny.py [backend/deny.toml] [--today YYYY-MM-DD]

An advisory that cargo-deny reports but that cannot be reached from this code may be ignored (`[advisories] ignore`), but not for good: every entry is a
table `{ id = "RUSTSEC-…", reason = "… review-by YYYY-MM-DD" }` (a bare string has no reason, a reason without a date has no end). Exit code 1 and the list
of what is wrong when an entry has no reason, no `review-by` date, or its date has passed: the ignore has to be looked at again (the crate may have a fix by now),
then either removed or given a new date and a new reason. cargo-deny keeps no dates itself; this is what makes the rule in SECURITY.md more than a comment.
The list is empty today, and the script then says so.
"""
import datetime
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT = ROOT / "backend" / "deny.toml"
REVIEW_BY = re.compile(r"review-by[:\s]+(\d{4}-\d{2}-\d{2})")


def problems(text, today):
    """What is wrong with the ignored advisories in the text of a deny.toml, as lines to print; `today` is a date."""
    ignored = tomllib.loads(text).get("advisories", {}).get("ignore", [])
    out = []
    for entry in ignored:
        if isinstance(entry, str):
            out.append(f"{entry}: ignored without a reason; write it as {{ id = \"{entry}\", reason = \"why it cannot be reached… review-by YYYY-MM-DD\" }}")
            continue
        name = entry.get("id") or entry.get("crate") or "(an entry with neither id nor crate)"
        reason = entry.get("reason", "").strip()
        if not reason:
            out.append(f"{name}: ignored without a reason")
            continue
        found = REVIEW_BY.search(reason)
        if not found:
            out.append(f"{name}: the reason has no \"review-by YYYY-MM-DD\" date, so nobody is made to look at the ignore again")
            continue
        try:
            date = datetime.date.fromisoformat(found.group(1))
        except ValueError:
            out.append(f"{name}: review-by {found.group(1)} is not a date")
            continue
        if date < today:
            out.append(f"{name}: review-by {date} has passed - look at the advisory again (is there a fix?), then remove the ignore or give it a new date and a new reason")
    return out, len(ignored)


def main(argv):
    today = datetime.date.today()
    args = list(argv)
    if "--today" in args:
        at = args.index("--today")
        today = datetime.date.fromisoformat(args[at + 1])
        del args[at : at + 2]
    path = pathlib.Path(args[0]) if args else DEFAULT
    found, count = problems(path.read_text(), today)
    for line in found:
        print(f"{path}: {line}")
    if found:
        return 1
    print(f"{path}: {count} ignored advisor{'y' if count == 1 else 'ies'}, each with a reason and a review date that has not passed" if count else f"{path}: no advisory is ignored")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
