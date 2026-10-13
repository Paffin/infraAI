#!/usr/bin/env python3
"""Refresh the release tables of the catalog (`catalog.versions`) from endoflife.date.

    python3 deploy/versions-sync.py                       # refresh every table in versions.json from its own `source`
    python3 deploy/versions-sync.py --add nats=nats-server --name nats="NATS"      # start tracking more products
    python3 deploy/versions-sync.py --check               # fail if a table is older than --max-stale-days

Each table keeps the URL it came from (`source`), so the server's "Sync" button in the admin panel and this tool do the
same thing: fetch the URL, keep the release lines that matter (still supported, or recent), write them sorted newest
first. Standard library only. Where the network needs a proxy or a custom CA, use the usual HTTPS_PROXY / SSL_CERT_FILE.
"""
import argparse
import datetime as dt
import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_FILE = os.path.join(HERE, "..", "backend", "crates", "engine", "defaults", "versions.json")
API = "https://endoflife.date/api"


def fetch(url, timeout):
    req = urllib.request.Request(url, headers={"User-Agent": "infra-architect-versions-sync", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def flag_or_date(v):
    """endoflife.date reports a date, true/false, or nothing."""
    if v is None:
        return None
    if isinstance(v, bool):
        return v
    s = str(v)
    return s[:10] if len(s) >= 10 and s[4] == "-" else None


def convert(rows, today, max_age_years):
    """The release lines of one product: still maintained or recent, newest first."""
    cutoff = today - dt.timedelta(days=int(365.25 * max_age_years))
    out = []
    for r in rows:
        cycle = str(r.get("cycle", "")).strip()
        if not cycle:
            continue
        eol = flag_or_date(r.get("eol"))
        released = flag_or_date(r.get("releaseDate"))
        over = eol is True or (isinstance(eol, str) and eol <= today.isoformat())
        if over and isinstance(released, str) and released < cutoff.isoformat():
            continue
        lts = r.get("lts")
        item = {"cycle": cycle}
        if isinstance(released, str):
            item["releaseDate"] = released
        support = flag_or_date(r.get("support"))
        if support is not None:
            item["support"] = support
        if eol is not None:
            item["eol"] = eol
        if r.get("latest"):
            item["latest"] = str(r["latest"])
        latest_date = flag_or_date(r.get("latestReleaseDate"))
        if isinstance(latest_date, str):
            item["latestReleaseDate"] = latest_date
        if lts:
            item["lts"] = True
        out.append(item)
    out.sort(key=lambda c: (c.get("releaseDate", ""), c["cycle"]), reverse=True)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--file", default=DEFAULT_FILE)
    ap.add_argument("--add", action="append", default=[], metavar="ID=PRODUCT", help="track a product: table id = endoflife.date product")
    ap.add_argument("--name", action="append", default=[], metavar="ID=NAME", help="how a tracked product is called in messages (kept once set)")
    ap.add_argument("--api", default=API, help="base URL of the API (default: %(default)s)")
    ap.add_argument("--max-age-years", type=float, default=8, help="drop release lines that ended more than this long ago")
    ap.add_argument("--timeout", type=float, default=20)
    ap.add_argument("--today", default=None, help="YYYY-MM-DD (default: today, UTC)")
    ap.add_argument("--check", action="store_true", help="only report tables older than --max-stale-days")
    ap.add_argument("--max-stale-days", type=int, default=45)
    a = ap.parse_args()

    today = dt.date.fromisoformat(a.today) if a.today else dt.datetime.now(dt.timezone.utc).date()
    with open(a.file, encoding="utf-8") as f:
        sets = json.load(f)
    by_id = {s["id"]: s for s in sets}
    for spec in a.add:
        sid, _, prod = spec.partition("=")
        if not sid or not prod:
            sys.exit(f"--add expects ID=PRODUCT, got {spec!r}")
        by_id.setdefault(sid, {"id": sid, "name": "", "source": f"{a.api}/{prod}.json", "asOf": "", "cycles": []})
    for spec in a.name:
        sid, _, nm = spec.partition("=")
        if sid not in by_id or not nm:
            sys.exit(f"--name expects ID=NAME for a tracked table, got {spec!r}")
        by_id[sid]["name"] = nm

    if a.check:
        stale = [s["id"] for s in by_id.values() if not s.get("asOf") or (today - dt.date.fromisoformat(s["asOf"])).days > a.max_stale_days]
        if stale:
            print("stale or empty tables:", ", ".join(sorted(stale)))
            sys.exit(1)
        print(f"{len(by_id)} tables, none older than {a.max_stale_days} days")
        return

    failed = []
    for sid in sorted(by_id):
        s = by_id[sid]
        url = s.get("source") or ""
        if not url.startswith("http"):
            continue  # a table edited by hand
        try:
            rows = fetch(url, a.timeout)
        except Exception as e:  # keep the old table: a failed refresh must not empty it
            failed.append((sid, str(e)))
            continue
        s["cycles"] = convert(rows, today, a.max_age_years)
        s["asOf"] = today.isoformat()
    result = [by_id[k] for k in sorted(by_id)]
    with open(a.file, "w", encoding="utf-8") as f:
        f.write(json.dumps(result, indent=1, ensure_ascii=False))
        f.write("\n")
    print(f"{len(result)} tables written to {os.path.relpath(a.file)}")
    for sid, err in failed:
        print(f"  not refreshed: {sid}: {err}", file=sys.stderr)
    if failed:
        sys.exit(2)


if __name__ == "__main__":
    main()
