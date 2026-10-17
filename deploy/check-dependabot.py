#!/usr/bin/env python3
"""Every directory that Dependabot can watch is watched: .github/dependabot.yml has an entry for it.

    python3 deploy/check-dependabot.py [root]        exit 1 and one line per directory that has no entry

Dependabot opens the pull requests that keep dependencies and pinned actions current; a directory it is not told about is never updated, and nothing says so: the
Dockerfile of the IaC check sat on `alpine/helm:3.16.2` while three newer releases came out. A new lock file, Dockerfile or workflow is a place that needs an entry,
and this fails the `docs` job until it has one (a new place is the moment somebody is looking).

What needs an entry (the ecosystem is the one Dependabot names it):

  cargo           a Cargo.lock, or a Cargo.toml that is the root of a workspace or a crate of its own; the crates of a workspace are covered by its root
  npm             a package-lock.json, or a package.json without one
  docker          a Dockerfile, Dockerfile.*, *.Dockerfile
  docker-compose  docker-compose*.y(a)ml, compose*.y(a)ml
  pip             a requirements*.txt, a pyproject.toml
  github-actions  the workflows (.github/workflows/*.yml) at `/`, and every composite action (.github/actions/*/action.yml) - `directories: ["/", "/.github/actions/*"]`

An entry covers the directories in `directory` and in `directories` (a `*` in a name stands for one directory level, as in Dependabot's own globs). node_modules, target,
dist and .git are not looked into; neither is deploy/tests/fixtures, whose trees are the test cases of this script.

What this cannot see: whether Dependabot understands what it finds. A `FROM ${REGISTRY}/node:${NODE_VERSION}-slim` has no tag for it to read, and an image named by a
variable of the compose file is not updated either; the entry exists, the pull requests do not. Look at Insights, Dependency graph, Dependabot after a change.
"""
import fnmatch
import glob
import os
import pathlib
import re
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "node_modules", "target", "dist", "build", "__pycache__", ".venv", "venv", "playwright-report", "test-results"}
SKIP_PREFIXES = ("deploy/tests/fixtures",)
DOCKERFILE = re.compile(r"^(Dockerfile(\..+)?|.+\.Dockerfile)$")
COMPOSE = re.compile(r"^(docker-)?compose([.-].+)?\.ya?ml$")
REQUIREMENTS = re.compile(r"^requirements([.-].+)?\.txt$")


def norm(directory):
    d = "/" + str(directory).strip("/")
    return d


def walk(root):
    """(directory relative to the root as '/a/b', [file names]) for every directory that is looked into."""
    for base, dirs, files in os.walk(root):
        rel = pathlib.Path(base).relative_to(root).as_posix()
        below = lambda d: d if rel == "." else f"{rel}/{d}"
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS and not below(d).startswith(SKIP_PREFIXES))
        yield norm("" if rel == "." else rel), sorted(files)


def has_workspace(text):
    return re.search(r"(?m)^\[workspace\]", text) is not None


def needs(root):
    """{(ecosystem, directory): why} for everything under the root that Dependabot can watch."""
    found = {}
    cargo_roots = []  # directories that hold a Cargo.toml with [workspace]
    tomls = {}
    for directory, files in walk(root):
        here = pathlib.Path(root) / directory.lstrip("/")
        if "Cargo.toml" in files:
            tomls[directory] = (here / "Cargo.toml").read_text(errors="replace")
            if has_workspace(tomls[directory]):
                cargo_roots.append(directory)
        if "Cargo.lock" in files:
            found[("cargo", directory)] = "Cargo.lock"
        if "package-lock.json" in files:
            found[("npm", directory)] = "package-lock.json"
        elif "package.json" in files:
            found.setdefault(("npm", directory), "package.json")
        for f in files:
            if DOCKERFILE.match(f):
                found.setdefault(("docker", directory), f)
            elif COMPOSE.match(f):
                found.setdefault(("docker-compose", directory), f)
            elif REQUIREMENTS.match(f):
                found.setdefault(("pip", directory), f)
        if "pyproject.toml" in files:
            found.setdefault(("pip", directory), "pyproject.toml")
    for directory, text in tomls.items():
        under_root = any(r != directory and (directory + "/").startswith(r.rstrip("/") + "/") for r in cargo_roots)
        if not under_root:  # the root of a workspace, or a crate on its own; the members of a workspace are the root's
            found.setdefault(("cargo", directory), "Cargo.toml")
    github = pathlib.Path(root) / ".github"
    if any((github / "workflows").glob("*.y*ml")) if (github / "workflows").is_dir() else False:
        found[("github-actions", "/")] = ".github/workflows"
    for action in sorted(github.glob("actions/*/action.y*ml")) if github.is_dir() else []:
        found[("github-actions", norm(action.parent.relative_to(root).as_posix()))] = action.relative_to(root).as_posix()
    return found


def covered(config):
    """[(ecosystem, [directory patterns])] of the entries of a dependabot.yml (its `updates`)."""
    out = []
    for entry in (config or {}).get("updates") or []:
        dirs = [entry["directory"]] if "directory" in entry else []
        dirs += entry.get("directories") or []
        out.append((entry.get("package-ecosystem"), [norm(d) for d in dirs]))
    return out


def check(root=ROOT):
    root = pathlib.Path(root)
    path = root / ".github" / "dependabot.yml"
    wanted = needs(root)
    if not path.exists():
        return [f".github/dependabot.yml does not exist, and there are {len(wanted)} place(s) for it to watch"], len(wanted), 0
    config = yaml.safe_load(path.read_text())
    entries = covered(config)
    problems = []
    for (eco, directory), why in sorted(wanted.items()):
        ok = any(e == eco and any(fnmatch.fnmatchcase(directory, pat) for pat in pats) for e, pats in entries)
        if not ok:
            problems.append(f"{eco} {directory}: has {why}, and .github/dependabot.yml has no `package-ecosystem: {eco}` entry for this directory")
    # an entry that watches nothing is a typo or a leftover: Dependabot errors on it ("directory not found") and the other entries go unwatched
    for eco, pats in entries:
        for pat in pats:
            if not any(e == eco and fnmatch.fnmatchcase(d, pat) for (e, d) in wanted) and not _exists(root, pat):
                problems.append(f"{eco} {pat}: the directory in .github/dependabot.yml does not exist")
    return problems, len(wanted), len(entries)


def _exists(root, pattern):
    return any(pathlib.Path(p).is_dir() for p in glob.glob(str(pathlib.Path(root) / pattern.lstrip("/"))))


def main(argv):
    root = pathlib.Path(argv[0]) if argv else ROOT
    problems, places, entries = check(root)
    for p in problems:
        print(p)
    print(f"{places} place(s) Dependabot can watch, {entries} entries in .github/dependabot.yml, {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
