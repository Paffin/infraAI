#!/usr/bin/env python3
"""Checks the kinds_*.py modules without writing anything: run it while a module is being written.

    python3 deploy/catalog/lint.py kinds_edge_a            # one module (everything else is read to find duplicates, but not reported)
    python3 deploy/catalog/lint.py                         # every module

Exit code 1 when there is an error. What is checked: ids, names, descriptions in both languages, functions, icons, traits that the
capacity model, the rules and the cost model need (capacity, replicas, encryption at rest, backup properties), release tables,
subtitles, variants. build.py runs the same checks over all modules.
"""
import json
import sys

import checks
import forge

names = [a for a in sys.argv[1:] if not a.startswith("-")]
mods = checks.modules()
broken = [(n, err) for n, _, err in mods if err]
for n, err in broken:
    print(f"{n}: cannot be imported – {err}")

cat = json.load(open(checks.CATALOG_JSON, encoding="utf-8"))
module_kinds = {n: list(getattr(m, "KINDS", [])) for n, m, _ in mods if m}
module_ids = {kd["id"] for ks in module_kinds.values() for kd in ks}
hand_written = [kd for kd in cat["kinds"] if kd["id"] not in module_ids]
fns_of = {c["id"]: {f["id"] for f in c.get("functions", [])} for c in cat["categories"]}
icons = checks.icon_names()
# a build puts the functions of the modules into catalog.json: they are the modules' own, not "already there"
declared = {fid for _, m, _ in mods if m for fns in getattr(m, "FUNCTIONS", {}).values() for fid, *_ in fns}
fns_of = {c: {f for f in fs if f not in declared} for c, fs in fns_of.items()}
for n, m, _ in mods:
    if not m:
        continue
    for cat_id, fns in getattr(m, "FUNCTIONS", {}).items():
        for fid, ru, en, icon in fns:
            if icon not in icons:
                print(f"{n}: function {fid}: icon {icon!r} is not in the registry of the UI (frontend/src/lib/icons.ts)")
                broken.append((n, "icon"))
            if any(fid in fs for fs in fns_of.values()):
                print(f"{n}: function {fid!r} is already declared (by the catalog or another module): declare it once")
                broken.append((n, "dup function"))
            fns_of.setdefault(cat_id, set()).add(fid)
known_traits = {t for kd in hand_written for t in kd["traits"] + [x for v in kd.get("variants", []) for x in v.get("addTraits", [])]} | {t for ts in forge.T.values() for t in ts} | checks.rule_traits()
all_kinds = hand_written + [kd for ks in module_kinds.values() for kd in ks]
only = {kd["id"] for n, ks in module_kinds.items() if not names or n in names for kd in ks}
errs, warns = checks.check_all(all_kinds, fns_of, tables=set(forge.VERS), known_traits=known_traits, icons=icons, only=only)
for n in names:
    if n not in module_kinds:
        print(f"{n}: no such module")
        broken.append((n, "missing"))
for n, ks in module_kinds.items():
    if names and n not in names:
        continue
    by_fn = {}
    for kd in ks:
        by_fn[kd["function"]] = by_fn.get(kd["function"], 0) + 1
    print(f"{n}: {len(ks)} kinds, {sum(len(kd.get('variants', [])) for kd in ks)} variants – " + ", ".join(f"{f} {c}" for f, c in sorted(by_fn.items())))
for w in warns:
    print("warning:", w)
for e in errs:
    print("ERROR:", e)
print(f"{len(errs)} errors, {len(warns)} warnings; the catalog would have {len(all_kinds)} kinds")
sys.exit(1 if errs or broken else 0)
