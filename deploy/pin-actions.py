#!/usr/bin/env python3
"""Pin the actions of the workflows by the commit they are at, the version in a comment.

    python3 deploy/pin-actions.py            # rewrites  uses: actions/checkout@v7  into  uses: actions/checkout@<40 hex> # v7.0.1
    python3 deploy/pin-actions.py --check    # offline: exit 1 and the list of every `uses:` that is not `@<40 hex> # vX.Y.Z` (what CI runs)
    python3 deploy/pin-actions.py --verify   # online: every pinned commit is the one its comment's tag points at (a pin copied wrong, a tag moved)

Why: a tag is a pointer that its owner can move, and one security scanner's tags were (March 2026): every run of every repository that said `@v1` then ran the
attacker's code with the repository's secrets. A commit cannot be moved. The comment is what Dependabot (`github-actions`, one weekly group) reads: it moves the
commit and the comment together, so the pins do not rot. `@v7` is rewritten to the HIGHEST tag of that major (what the floating tag resolved to anyway), `@v7.0.1`
to exactly that tag.

Traps this handles: an ANNOTATED tag points at a tag object, not at the commit: the line `refs/tags/v2.9.2^{}` of `git ls-remote` is the commit, the line without
`^{}` is not (pinning the other one gives "unable to find version" at run time). A branch (`@stable`, `@main`) is not a version: it is refused, pin it by hand.
Local actions (`./.github/actions/x`) are in the repository and need no pin; `docker://image@sha256:…` is already a digest.

Where the tags are read from: https://github.com/{repo}.git, or $PIN_ACTIONS_REMOTE (a template with `{repo}`; the tests point it at real repositories on disk).
"""
import argparse
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FILES = ["workflows/*.yml", "workflows/*.yaml", "actions/*/action.yml", "actions/*/action.yaml"]
LOCAL = re.compile(r"^\s*(?:-\s+)?uses:\s*\./\S*\s*(?:#.*)?$")
USES = re.compile(r"^(?P<head>\s*(?:-\s+)?uses:\s*)(?P<action>[^\s@#'\"]+)@(?P<ref>[^\s#'\"]+)(?P<comment>\s*#.*)?\s*$")
SHA = re.compile(r"^[0-9a-f]{40}$")
VERSION_COMMENT = re.compile(r"^\s*#\s*(v\d+\.\d+\.\d+)\s*$")
SEMVER_TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
REMOTE = os.environ.get("PIN_ACTIONS_REMOTE", "https://github.com/{repo}.git")


def workflow_files(root=ROOT):
    github = pathlib.Path(root) / ".github"
    return sorted(p for pattern in FILES for p in github.glob(pattern))


def repo_of(action):
    """`owner/repo` of an action path (`owner/repo/sub/dir` is a directory of the same repository)."""
    return "/".join(action.split("/")[:2])


def uses_lines(text):
    """(line number from 1, match) for every `uses:` line; a `uses:` the pattern cannot read (quoted, an expression) is returned with match None."""
    for n, line in enumerate(text.splitlines(), 1):
        if re.match(r"^\s*(?:-\s+)?uses:", line) and not LOCAL.match(line):
            yield n, USES.match(line)


def problems(text):
    """What is not pinned in the text of one file: [(line number, sentence)]."""
    out = []
    for n, m in uses_lines(text):
        if m is None:
            raw = re.match(r"^\s*(?:-\s+)?uses:\s*(\S+)", text.splitlines()[n - 1])
            if raw and raw.group(1).startswith("docker://"):
                if not re.search(r"@sha256:[0-9a-f]{64}$", raw.group(1)):
                    out.append((n, f"{raw.group(1)}: a docker image is pinned by its digest (@sha256:…)"))
                continue
            out.append((n, "cannot be read: write `uses: owner/repo@<40 hex> # vX.Y.Z` without quotes or expressions"))
            continue
        action, ref, comment = m["action"], m["ref"], m["comment"] or ""
        if action.startswith("docker://"):
            if not (ref.startswith("sha256:") and re.fullmatch(r"sha256:[0-9a-f]{64}", ref)):
                out.append((n, f"{action}@{ref}: a docker image is pinned by its digest (@sha256:…)"))
            continue
        if not SHA.match(ref):
            out.append((n, f"{action}@{ref}: pinned by a tag or a branch, not by a commit (python3 deploy/pin-actions.py)"))
        elif not VERSION_COMMENT.match(comment):
            out.append((n, f"{action}@{ref[:12]}…: the version is missing; the comment must read `# vX.Y.Z` (Dependabot moves the commit and the comment together)"))
    return out


class Tags:
    """The tags of one repository, read with `git ls-remote`: {tag: commit} (an annotated tag is dereferenced)."""

    def __init__(self):
        self.cache = {}

    def of(self, repo):
        if repo not in self.cache:
            url = REMOTE.format(repo=repo)
            p = subprocess.run(["git", "ls-remote", "--tags", url], capture_output=True, text=True, timeout=120, env={**os.environ, "GIT_TERMINAL_PROMPT": "0"})
            if p.returncode != 0:
                raise SystemExit(f"cannot read the tags of {repo} ({url}): {p.stderr.strip()[-300:]}")
            tags = {}
            for line in p.stdout.splitlines():
                sha, _, ref = line.partition("\t")
                if not ref.startswith("refs/tags/"):
                    continue
                name = ref[len("refs/tags/"):]
                if name.endswith("^{}"):
                    tags[name[:-3]] = sha  # the commit an annotated tag points at wins over the tag object
                else:
                    tags.setdefault(name, sha)
            self.cache[repo] = tags
        return self.cache[repo]


def resolve(tags, ref):
    """(tag, commit) for a ref like v7, v7.1 or v7.0.1: the highest `vX.Y.Z` that starts with it."""
    want = ref[1:] if ref.startswith("v") else ref
    parts = want.split(".")
    if not ref.startswith("v") or not all(p.isdigit() for p in parts) or not 1 <= len(parts) <= 3:
        raise ValueError(f"@{ref} is not a version tag (a branch moves): pin it by hand")
    best = None
    for name, sha in tags.items():
        m = SEMVER_TAG.match(name)
        if not m or [m[i + 1] for i in range(len(parts))] != parts:
            continue
        key = tuple(int(x) for x in m.groups())
        if best is None or key > best[0]:
            best = (key, name, sha)
    if best is None:
        raise ValueError(f"no tag vX.Y.Z starts with {ref}")
    return best[1], best[2]


def tag_of(tags, sha):
    """The highest semver tag at this commit, to write the comment of an old pin that has none."""
    names = sorted((tuple(int(x) for x in SEMVER_TAG.match(n).groups()), n) for n, s in tags.items() if s == sha and SEMVER_TAG.match(n))
    return names[-1][1] if names else None


def rewrite(text, tags):
    """The text with every unpinned `uses:` pinned; (new text, [what was done], [what could not be]). `tags` is a Tags."""
    done, failed, out = [], [], []
    for n, line in enumerate(text.split("\n"), 1):
        m = USES.match(line) if re.match(r"^\s*(?:-\s+)?uses:", line) and not LOCAL.match(line) else None
        if m and not m["action"].startswith(("./", "docker://")):
            action, ref, comment = m["action"], m["ref"], m["comment"] or ""
            try:
                if not SHA.match(ref):
                    tag, sha = resolve(tags.of(repo_of(action)), ref)
                    line = f"{m['head']}{action}@{sha} # {tag}"
                    done.append(f"line {n}: {action}@{ref} -> {sha[:12]} # {tag}")
                elif not VERSION_COMMENT.match(comment):
                    tag = tag_of(tags.of(repo_of(action)), ref)
                    if tag is None:
                        raise ValueError(f"no tag vX.Y.Z points at {ref[:12]}")
                    line = f"{m['head']}{action}@{ref} # {tag}"
                    done.append(f"line {n}: {action}@{ref[:12]} commented # {tag}")
            except ValueError as e:
                failed.append(f"line {n}: {action}@{ref}: {e}")
        out.append(line)
    return "\n".join(out), done, failed


def verify(text, tags):
    """[(line number, sentence)] for the pins whose comment names a tag that does not point at the pinned commit."""
    bad = []
    for n, m in uses_lines(text):
        if not m or m["action"].startswith(("./", "docker://")) or not SHA.match(m["ref"]):
            continue
        c = VERSION_COMMENT.match(m["comment"] or "")
        if not c:
            continue
        known = tags.of(repo_of(m["action"]))
        if c.group(1) not in known:
            bad.append((n, f"{m['action']}: the tag {c.group(1)} does not exist"))
        elif known[c.group(1)] != m["ref"]:
            bad.append((n, f"{m['action']}: {c.group(1)} is {known[c.group(1)][:12]}…, the pin is {m['ref'][:12]}…"))
    return bad


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="offline: list what is not pinned, exit 1 if anything")
    mode.add_argument("--verify", action="store_true", help="online: the pinned commits are the commits of the tags in the comments")
    ap.add_argument("--root", default=str(ROOT), help="the repository (default: this one)")
    args = ap.parse_args(argv)
    files = workflow_files(args.root)
    root = pathlib.Path(args.root)
    count = sum(1 for f in files for _, m in uses_lines(f.read_text()) if m and not m["action"].startswith(("./", "docker://")))
    bad = 0
    tags = Tags()
    for f in files:
        text = f.read_text()
        rel = f.relative_to(root)
        if args.check:
            found = problems(text)
        elif args.verify:
            found = problems(text) + verify(text, tags)
        else:
            new, done, failed = rewrite(text, tags)
            for line in done:
                print(f"{rel}: {line}")
            if new != text:
                f.write_text(new)
            found = [(0, x) for x in failed]
        for n, what in found:
            bad += 1
            print(f"{rel}:{n}: {what}")
    if args.check or args.verify:
        print(f"{count} action reference(s) in {len(files)} file(s), {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
