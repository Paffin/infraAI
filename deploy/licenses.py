#!/usr/bin/env python3
"""What this project ships that others wrote, and the check that all of it carries a license the project accepts.

    python3 deploy/licenses.py            # rewrite THIRD_PARTY_LICENSES.md
    python3 deploy/licenses.py --check    # exit 1 if the file is out of date or something carries a license that is not accepted (used in CI)
    python3 deploy/licenses.py --versions # print every shipped component with its exact version and license (what the lock files pin)

Rust crates come from `cargo metadata` (what is linked into the binary: the normal dependencies of the workspace; test and build tools are not shipped),
npm packages from frontend/package-lock.json (production dependencies, which the build bundles into the web application). The accepted licenses and the
platform the crates are resolved for are in deploy/licenses.json.

The committed list names each component once, with its license, and carries no versions (those are in the lock files): a dependency update that changes
nothing about what is shipped or under which license leaves the file as it is, so that Dependabot's pull requests are not red for a reason that is not theirs.
The license policy itself is checked against the live lock files, whatever the file says: a new version that changes its license fails the check at once. The check also keeps the project's own license in one piece: the same SPDX identifier in
the Cargo workspace and in package.json, the Apache-2.0 text in LICENSE, a NOTICE.
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
POLICY = json.loads((ROOT / "deploy/licenses.json").read_text())
OUT = ROOT / "THIRD_PARTY_LICENSES.md"
OWN = "Apache-2.0"


# ---------------------------------------------------------------------------------------------------- license expressions
def tokens(expr):
    return re.findall(r"\(|\)|[^\s()]+", expr.replace("/", " OR "))


def satisfied(expr, allowed):
    """Can the SPDX expression be met with the accepted licenses? (OR: any term, AND: all terms, `X WITH exception`: X, `X+`: X)"""
    toks = tokens(expr)
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def take():
        nonlocal pos
        pos += 1
        return toks[pos - 1]

    def factor():
        t = take()
        if t == "(":
            v = parse_or()
            if peek() == ")":
                take()
            return v
        t = t.rstrip("+")
        ok = t in allowed
        if peek() == "WITH":
            take()
            take()
        return ok

    def parse_and():
        v = factor()
        while peek() == "AND":
            take()
            v = factor() and v
        return v

    def parse_or():
        v = parse_and()
        while peek() == "OR":
            take()
            v = parse_and() or v
        return v

    return bool(toks) and parse_or()


def canonical(expr):
    """The same alternatives are written in different orders and with `/` by different authors: one spelling for the summary."""
    expr = re.sub(r"\s*/\s*", " OR ", expr.strip())
    if re.search(r"[()]| AND | WITH ", expr):
        return expr
    return " OR ".join(sorted(set(expr.split(" OR "))))


# ---------------------------------------------------------------------------------------------------- components
def rust_components():
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", POLICY["cargoTarget"]]
    run = subprocess.run(cmd, cwd=ROOT / "backend", capture_output=True, text=True)
    if run.returncode != 0:
        run = subprocess.run(cmd + ["--offline"], cwd=ROOT / "backend", capture_output=True, text=True)
    if run.returncode != 0:
        sys.exit("cargo metadata failed:\n" + run.stderr[-2000:])
    meta = json.loads(run.stdout)
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    seen, todo = set(), list(members)
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k.get("kind") is None for k in d["dep_kinds"]):
                todo.append(d["pkg"])
    own = [pkgs[i] for i in members]
    shipped = [pkgs[i] for i in seen if i not in members]
    return own, [{"name": p["name"], "version": p["version"], "license": canonical(p.get("license") or "UNKNOWN"), "url": f"https://crates.io/crates/{p['name']}"} for p in shipped]


def npm_components():
    lock = json.loads((ROOT / "frontend/package-lock.json").read_text())
    out = []
    for key, v in lock["packages"].items():
        if not key or v.get("dev"):
            continue
        name = key.rsplit("node_modules/", 1)[-1]
        lic = v.get("license")
        if isinstance(lic, dict):
            lic = lic.get("type")
        out.append({"name": name, "version": v.get("version", "?"), "license": canonical(lic or "UNKNOWN"), "url": f"https://www.npmjs.com/package/{name}"})
    return lock["packages"][""], out


# ---------------------------------------------------------------------------------------------------- the document
def collapse(items):
    """One entry per component and license: a crate that is linked in two versions (the RustCrypto generations side by side) is one line."""
    out = {}
    for c in items:
        out.setdefault((c["name"], c["license"]), {"name": c["name"], "license": c["license"], "url": c["url"], "versions": set()})["versions"].add(c["version"])
    return sorted(out.values(), key=lambda c: (c["name"].lower(), c["license"]))


def table(items):
    rows = ["| Component | License |", "|---|---|"]
    for c in items:
        rows.append(f"| [{c['name']}]({c['url']}) | {c['license'].replace('|', '/')} |")
    return "\n".join(rows)


def summary(rust, npm):
    by = {}
    for eco, items in (("rust", rust), ("npm", npm)):
        for c in items:
            by.setdefault(c["license"], {"rust": 0, "npm": 0})[eco] += 1
    rows = ["| License expression | Rust crates | npm packages |", "|---|---|---|"]
    for lic, n in sorted(by.items(), key=lambda kv: (-(kv[1]["rust"] + kv[1]["npm"]), kv[0])):
        rows.append(f"| {lic.replace('|', '/')} | {n['rust'] or ''} | {n['npm'] or ''} |")
    return "\n".join(rows)


def document(rust, npm):
    notes = "\n".join(f"* **{k}**: {v}" for k, v in POLICY["notes"].items())
    allowed = ", ".join(f"`{a}`" for a in POLICY["allowed"])
    return f"""# Third-party licenses

Infra Architect is licensed under the [Apache License 2.0](LICENSE); see [NOTICE](NOTICE). The server binary and the web application include the
components below, each under its own license. This file is generated by `python3 deploy/licenses.py` from `backend/Cargo.lock` and
`frontend/package-lock.json` (what is shipped: the normal dependencies of the server, the production dependencies of the web application), and
`python3 deploy/licenses.py --check` keeps it current and fails when a component carries a license outside the accepted ones. Versions are not listed
here (they are in the lock files, and a different version of the same component does not change this list); `python3 deploy/licenses.py --versions` prints them.

Accepted licenses (`deploy/licenses.json`): {allowed}.

{notes}

## Summary

{summary(rust, npm)}

## Rust crates ({len(rust)})

{table(rust)}

## npm packages ({len(npm)})

{table(npm)}
"""


# ---------------------------------------------------------------------------------------------------- checks
def own_problems(own_crates, npm_root):
    problems = []
    text = (ROOT / "LICENSE").read_text() if (ROOT / "LICENSE").exists() else ""
    if "Apache License" not in text or "Version 2.0, January 2004" not in text:
        problems.append("LICENSE is not the Apache License 2.0")
    if not (ROOT / "NOTICE").exists():
        problems.append("NOTICE is missing")
    for p in own_crates:
        if p.get("license") != OWN:
            problems.append(f"crate {p['name']} declares license {p.get('license')!r}, the project's is {OWN!r}")
    if npm_root.get("license") != OWN:
        problems.append(f"frontend/package.json declares license {npm_root.get('license')!r}, the project's is {OWN!r}")
    return problems


def main():
    check = "--check" in sys.argv[1:]
    own_crates, rust = rust_components()
    npm_root, npm = npm_components()
    problems = own_problems(own_crates, npm_root)
    allowed = set(POLICY["allowed"])
    for eco, items in (("crate", rust), ("npm package", npm)):
        for c in items:
            if not satisfied(c["license"], allowed):
                problems.append(f"{eco} {c['name']} {c['version']}: license {c['license']!r} is not among the accepted ones (deploy/licenses.json)")
    if "--versions" in sys.argv[1:]:
        for eco, items in (("crate", rust), ("npm", npm)):
            for c in sorted(items, key=lambda c: (c["name"].lower(), c["version"])):
                print(f"{eco}\t{c['name']}\t{c['version']}\t{c['license']}")
        return
    text = document(collapse(rust), collapse(npm))
    stale = not OUT.exists() or OUT.read_text() != text
    if problems:
        print("\n".join(problems), file=sys.stderr)
    if check:
        if stale:
            print("THIRD_PARTY_LICENSES.md is out of date: run python3 deploy/licenses.py", file=sys.stderr)
        sys.exit(1 if problems or stale else 0)
    OUT.write_text(text)
    print(f"THIRD_PARTY_LICENSES.md: {len(collapse(rust))} Rust crates, {len(collapse(npm))} npm packages" + ("" if not problems else f"; {len(problems)} problem(s)"))
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
