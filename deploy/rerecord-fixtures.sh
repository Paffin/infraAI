#!/usr/bin/env bash
# Re-record the frontend fixtures that depend on the shipped release tables (versions.json):
#   * lifecycle.json       – the engine's verdicts for versions at three dates (the editor's port of the arithmetic is tested against them)
#   * report-versions.json – the real report of an architecture with expired, expiring and mixed versions
#
# Run it after `python3 deploy/versions-sync.py` (the weekly "Release tables" job does). FIXTURE_TODAY (default 2026-10-13) is the
# date the engine is given for the report: it stays fixed, so only changes of the tables move the fixtures.
# Needs a Rust toolchain.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
FIX="$ROOT/frontend/tests/fixtures"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

(cd "$ROOT/backend" && cargo run -q -p engine --example lifecycle) > "$TMP/lifecycle.json"
mv "$TMP/lifecycle.json" "$FIX/lifecycle.json"

python3 - "$FIX/report-versions.json" "$TMP/arch.json" <<'PY'
import json, sys
json.dump(json.load(open(sys.argv[1]))["architecture"], open(sys.argv[2], "w"), ensure_ascii=False)
PY
FIXTURE_TODAY="${FIXTURE_TODAY:-2026-10-13}" "$ROOT/deploy/record-fixture.sh" "$TMP/arch.json" report-versions
