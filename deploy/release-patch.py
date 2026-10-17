#!/usr/bin/env python3
"""The hand-over between the two jobs of .github/workflows/versions.yml: what the refresh produced, and the check that only that is committed.

    python3 deploy/release-patch.py make  PATCH      in `refresh`: write the changes of the working tree that are allowed into PATCH
    python3 deploy/release-patch.py apply PATCH      in `commit`: refuse the patch if it touches anything else, otherwise apply it to the index

Why two jobs: `refresh` downloads data, runs cargo (build scripts, proc macros), `npm ci` (lifecycle scripts) and the repository's own tests - code from a hundred
sources - and none of it may hold the token that can push to main. `commit` has that token and runs nothing but this script and git. What crosses between them is a
patch, and a patch can say anything: a dependency that went wrong could make `refresh` emit one that rewrites a workflow, a script or a source file, and `commit` would
push it. So `apply` does not trust it: every path the patch touches must be one the refresh is meant to write (ALLOWED), nothing may be renamed, copied, made executable,
turned into a symlink or deleted. Anything else fails the run and main stays as it was.

ALLOWED is the list of what `deploy/versions-sync.py`, `deploy/catalog/build.py`, `deploy/gen-docs.py` and `deploy/rerecord-fixtures.sh` write. A new thing the refresh
writes is a failed run with the path in the message: add it here, in the same commit that makes the refresh write it.
"""
import re
import subprocess
import sys

ALLOWED = [
    "backend/crates/engine/defaults/versions.json",
    "backend/crates/engine/defaults/catalog.json",
    "backend/crates/engine/defaults/iac/*.json",
    "docs/*.md",
    "README.md",
    "README.en.md",
    "frontend/tests/fixtures/lifecycle.json",
    "frontend/tests/fixtures/report-versions.json",
]
REGULAR_FILE = "100644"


def _pattern(glob):
    return re.compile("".join("[^/]*" if part == "*" else re.escape(part) for part in re.split(r"(\*)", glob)) + r"\Z")


_ALLOWED = [_pattern(g) for g in ALLOWED]


def allowed(path):
    """Is this path (relative to the root, with `/`) one the refresh writes? `*` stands for a name, never for a directory."""
    return any(p.match(path) for p in _ALLOWED)


def git(*args, input=None, check=True):
    p = subprocess.run(["git", *args], input=input, capture_output=True, check=False)
    if check and p.returncode != 0:
        sys.exit(f"git {' '.join(args[:2])} failed: {p.stderr.decode('utf-8', 'replace').strip()[-400:]}")
    return p


def paths_of(patch):
    """What Git itself would touch when it applies the patch: ([paths], [objections]); the objections are renames, copies, modes and deletions."""
    numstat = git("apply", "--numstat", "-z", "--", patch).stdout.decode("utf-8", "surrogateescape")
    paths, objections = [], []
    # with -z every file is "added\tdeleted\tpath" NUL; a rename or a copy is "added\tdeleted\t" NUL old NUL new NUL
    fields = numstat.split("\0")
    i = 0
    while i < len(fields) and fields[i]:
        m = re.match(r"^(\S+)\t(\S+)\t(.*)$", fields[i], re.S)
        i += 1
        if not m:
            objections.append(f"cannot be read: {fields[i - 1][:80]!r}")
            break
        if m.group(3) == "":
            old, new = fields[i], fields[i + 1]
            i += 2
            objections.append(f"renames or copies {old} to {new}")
            paths += [old, new]
        else:
            paths.append(m.group(3))
    summary = git("apply", "--summary", "--", patch).stdout.decode("utf-8", "surrogateescape")
    for line in summary.splitlines():
        line = line.strip()
        if line.startswith(("rename ", "copy ")):
            objections.append(f"renames or copies: {line}")
        elif line.startswith("mode change"):
            objections.append(f"changes a mode: {line}")
        elif line.startswith("delete mode"):
            objections.append(f"deletes a file ({line}): the refresh does not delete anything")
        elif line.startswith("create mode") and f" {REGULAR_FILE} " not in line:
            objections.append(f"creates something that is not a regular file ({line})")
    return sorted(set(paths)), objections


def apply(patch):
    with open(patch, "rb") as f:
        if not f.read().strip():
            print("the patch is empty: nothing to commit")
            return 0
    paths, objections = paths_of(patch)
    outside = [p for p in paths if not allowed(p)]
    if outside or objections:
        for p in outside:
            print(f"::error title=Release tables::the patch touches {p}, which the refresh does not write (deploy/release-patch.py, ALLOWED)")
        for o in objections:
            print(f"::error title=Release tables::the patch {o}")
        print(f"refused: {len(outside)} path(s) outside the list, {len(objections)} other objection(s); nothing was applied")
        return 1
    # all or nothing: if main has moved under the patch since the refresh started (the same lines changed), nothing is applied and the run fails; next week's run starts again
    git("apply", "--index", "--", patch)
    print(f"applied: {len(paths)} file(s)")
    for p in paths:
        print(f"  {p}")
    return 0


def make(patch):
    """Stage the changes of the working tree that are allowed, write them as a patch, and say what was left out."""
    status = git("status", "--porcelain", "-z", "--untracked-files=all").stdout.decode("utf-8", "surrogateescape").split("\0")
    changed, i = [], 0
    while i < len(status) and status[i]:
        entry = status[i]
        code, path = entry[:2], entry[3:]
        i += 2 if code[0] in "RC" else 1  # a rename lists the old name after the new one
        changed.append(path)
    keep = [p for p in changed if allowed(p)]
    left = [p for p in changed if not allowed(p)]
    for p in left:
        print(f"::warning title=Release tables::{p} changed during the refresh and is not part of the commit (not in ALLOWED of deploy/release-patch.py)")
    if keep:
        git("add", "--", *keep)
    out = git("diff", "--cached", "--binary", "--no-renames").stdout
    with open(patch, "wb") as f:
        f.write(out)
    print(f"{len(keep)} file(s) in the patch, {len(left)} left out")
    for p in keep:
        print(f"  {p}")
    return 0


def main(argv):
    if len(argv) != 2 or argv[0] not in ("make", "apply"):
        sys.exit(__doc__)
    return {"make": make, "apply": apply}[argv[0]](argv[1])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
