#!/usr/bin/env python3
"""Did every browser test of the suite run, once, and pass?

    python3 deploy/check-e2e-report.py list.json merged.json

`list.json` is `npx playwright test --list --reporter=json` (every test there is), `merged.json` is `npx playwright merge-reports --reporter=json <blobs>` of the
shards (what ran). ci.yml's `e2e-report` job runs it after the shards have merged: a test is silently lost when a shard crashes before it writes its report, when
the matrix is changed and its parts no longer add up to the whole, or when a test is skipped by a stray `test.skip`/`test.fixme`/`test.only` - and a green run
that did not run 40 of 400 tests is worse than a red one. Exit code 1 and the names when:

  * a listed test did not run (no shard had it), or ran twice (two shards had it), or ran but is not listed;
  * a test was skipped;
  * a test failed (the shards are red then, too; this says it in one place);
  * the totals of the report (`stats`) differ from the tests it contains.

A test that passed on a retry (`flaky`) counts as run and is named in a `::warning::`, so that it is seen.
"""
import collections
import json
import pathlib
import sys

MAX_NAMES = 25


def walk(node, path=()):
    """(path of titles, spec) for every spec under a suite."""
    for spec in node.get("specs", []):
        yield path, spec
    for child in node.get("suites", []):
        yield from walk(child, path + (child.get("title", ""),))


def tests_of(report):
    """{spec id: (title, [status of each test of the spec])}; a spec has one test per project."""
    found = {}
    for top in report.get("suites", []):
        for path, spec in walk(top, (top.get("title", ""),)):
            key = spec.get("id") or "/".join(path + (spec.get("title", ""),))
            name = " > ".join(p for p in path + (spec.get("title", ""),) if p)
            found.setdefault(key, (name, []))[1].extend(t.get("status", "?") for t in spec.get("tests", []))
    return found


def verdict(listed, merged):
    """(problems, warnings, summary) from the two parsed JSON documents."""
    want, got = tests_of(listed), tests_of(merged)
    problems, warnings = [], []

    def names(keys, table):
        shown = [table[k][0] for k in sorted(keys, key=lambda k: table[k][0])]
        return "; ".join(shown[:MAX_NAMES]) + (f"; and {len(shown) - MAX_NAMES} more" if len(shown) > MAX_NAMES else "")

    total_listed = sum(len(s) for _, s in want.values())
    status = collections.Counter(s for _, ss in got.values() for s in ss)
    ran = status["expected"] + status["unexpected"] + status["flaky"]

    missing = set(want) - set(got)
    if missing:
        problems.append(f"{len(missing)} listed test(s) did not run (a shard that never wrote its report?): {names(missing, want)}")
    unlisted = set(got) - set(want)
    if unlisted:
        problems.append(f"{len(unlisted)} test(s) ran that `--list` does not show: {names(unlisted, got)}")
    # a spec that has more results than it has projects ran twice (two shards had it)
    twice = {k for k in set(want) & set(got) if len(got[k][1]) > len(want[k][1])}
    if twice:
        problems.append(f"{len(twice)} test(s) ran more than once (two shards overlap?): {names(twice, got)}")
    skipped = {k for k, (_, ss) in got.items() if "skipped" in ss}
    if skipped:
        problems.append(f"{len(skipped)} test(s) were skipped (a test.skip, test.fixme or a failed precondition: a skipped test proves nothing): {names(skipped, got)}")
    failed = {k for k, (_, ss) in got.items() if "unexpected" in ss}
    if failed:
        problems.append(f"{len(failed)} test(s) failed: {names(failed, got)}")
    flaky = {k for k, (_, ss) in got.items() if "flaky" in ss}
    if flaky:
        warnings.append(f"{len(flaky)} test(s) passed only on a retry: {names(flaky, got)}")
    if ran != total_listed and not (missing or unlisted or twice or skipped):
        problems.append(f"{ran} tests ran, {total_listed} are listed")

    stats = merged.get("stats") or {}
    for field, count in [("expected", status["expected"]), ("unexpected", status["unexpected"]), ("flaky", status["flaky"]), ("skipped", status["skipped"])]:
        if field in stats and stats[field] != count:
            problems.append(f"the report says {stats[field]} {field}, the tests in it are {count}")
    summary = f"{total_listed} listed, {ran} ran ({status['expected']} passed, {status['flaky']} flaky, {status['unexpected']} failed), {status['skipped']} skipped"
    return problems, warnings, summary


def main(argv):
    if len(argv) != 2:
        sys.exit(__doc__)
    listed, merged = (json.loads(pathlib.Path(p).read_text()) for p in argv)
    problems, warnings, summary = verdict(listed, merged)
    print(summary)
    for w in warnings:
        print(f"::warning title=Flaky browser test::{w}")
    for p in problems:
        print(f"::error title=Browser tests::{p}")
        print(f"FAIL {p}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
