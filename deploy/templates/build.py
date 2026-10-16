#!/usr/bin/env python3
"""Writes the reference architectures that are written in code (deploy/templates/t_*.py) as the JSON the engine ships with.

    python3 deploy/templates/build.py            # write backend/crates/engine/defaults/templates/<id>.json
    python3 deploy/templates/build.py --check    # exit 1 if a file is out of date or a template names what the catalog does not have (used in CI)

The first eight reference architectures were written as JSON and stay that way; the ones written here are checked against the catalog before they are written
(kinds, variants, zone and link types, links between nodes that exist). Whether a design is *good* is the engine's verdict: `cargo test` runs every shipped
design through the rules, the failure simulator, the availability estimate and the gate.
"""
import importlib.util
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from lib import OUT  # noqa: E402

CATALOG = json.loads((HERE.parent.parent / "backend/crates/engine/defaults/catalog.json").read_text())
KINDS = {k["id"]: k for k in CATALOG["kinds"]}
ZONE_TYPES = {z["id"] for z in CATALOG["zoneTypes"]}
EDGE_TYPES = {e["id"] for e in CATALOG["edgeTypes"]}


def lint(a):
    errs = []
    zones = {z["id"] for z in a.zones}
    nodes = {n["id"] for n in a.nodes}
    for z in a.zones:
        if z["type"] not in ZONE_TYPES:
            errs.append(f"zone {z['id']}: unknown type {z['type']!r}")
        if z["parentId"] is not None and z["parentId"] not in zones:
            errs.append(f"zone {z['id']}: unknown parent {z['parentId']!r}")
    for n in a.nodes:
        k = KINDS.get(n["kind"])
        if not k:
            errs.append(f"node {n['id']}: unknown kind {n['kind']!r}")
            continue
        if n["zoneId"] is not None and n["zoneId"] not in zones:
            errs.append(f"node {n['id']}: unknown zone {n['zoneId']!r}")
        v = n["props"].get("variant")
        if v is not None and v not in {x["id"] for x in k.get("variants", [])}:
            errs.append(f"node {n['id']}: kind {n['kind']} has no variant {v!r}")
        # a variant may bring its own roles (Vault on Raft has voters, not a primary and replicas)
        var = next((x for x in k.get("variants", []) if x["id"] == v), None)
        roles = {r["id"] for r in ((var or {}).get("roles") or k.get("roles") or [])}
        if n["role"] is not None and roles and n["role"] not in roles:
            errs.append(f"node {n['id']}: kind {n['kind']} has no role {n['role']!r} (it has {sorted(roles)})")
    for e in a.edges:
        for end in ("source", "target"):
            if e[end] not in nodes:
                errs.append(f"edge {e['id']}: unknown {end} {e[end]!r}")
        if e["type"] not in EDGE_TYPES:
            errs.append(f"edge {e['id']}: unknown type {e['type']!r}")
    for key in ("rpsPeak", "rtoSeconds", "rpoSeconds", "availabilitySlo", "failureTolerance"):
        if key not in a.requirements:
            errs.append(f"requirements: {key} is not set (every shipped design carries its targets)")
    return errs


def load():
    out = []
    for path in sorted(HERE.glob("t_*.py")):
        spec = importlib.util.spec_from_file_location(path.stem, path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        out.append((path, mod.TEMPLATE))
    return out


def main():
    check = "--check" in sys.argv[1:]
    stale, problems, orders = [], [], {}
    for path, a in load():
        if a.id != path.stem[2:].replace("_", "-"):
            problems.append(f"{path.name}: the id {a.id!r} does not match the file name")
        problems += [f"{a.id}: {e}" for e in lint(a)]
        if a.order in orders:
            problems.append(f"{a.id}: order {a.order} is also {orders[a.order]}'s")
        orders[a.order] = a.id
        target = OUT / f"{a.id}.json"
        text = a.text()
        if check:
            if not target.exists() or target.read_text() != text:
                stale.append(target.name)
        else:
            target.write_text(text)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)
    if stale:
        print("out of date: " + ", ".join(stale) + "\nrun: python3 deploy/templates/build.py", file=sys.stderr)
        sys.exit(1)
    print(f"{len(orders)} reference architectures written in code" + (" – up to date" if check else ""))


if __name__ == "__main__":
    main()
