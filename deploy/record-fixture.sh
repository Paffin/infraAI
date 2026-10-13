#!/usr/bin/env bash
# Record a frontend test fixture from the REAL engine:
#
#   deploy/record-fixture.sh <template-id | path/to/architecture.json> <fixture-name>
#
# Writes frontend/tests/fixtures/<fixture-name>.json = {"architecture": …, "report": …}, where `report` is what
# `engine::evaluate::evaluate` returns (shipped catalog, rules and settings) for that architecture – the frontend
# suites (needs, objectives, capacity, diagram) run on these files instead of on hand-written look-alikes.
# Needs a Rust toolchain; run it again only when the shape of the report changes.
set -euo pipefail
SRC=${1:?usage: record-fixture.sh <template-id | architecture.json> <fixture-name>}
NAME=${2:?usage: record-fixture.sh <template-id | architecture.json> <fixture-name>}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/frontend/tests/fixtures/$NAME.json"
mkdir -p "$ROOT/frontend/tests/fixtures"
(cd "$ROOT/backend" && cargo run -q -p engine --example report -- "$SRC" en --json) > "$OUT.report"
python3 - "$SRC" "$OUT" "$ROOT" <<'PY'
import json, os, sys
src, out, root = sys.argv[1:4]
if os.path.exists(src):
    arch = json.load(open(src))
else:
    arch = json.load(open(f"{root}/backend/crates/engine/defaults/templates/{src}.json"))["architecture"]
report = json.load(open(out + ".report"))
json.dump({"architecture": arch, "report": report}, open(out, "w"), ensure_ascii=False, indent=1)
os.remove(out + ".report")
PY
echo "recorded $OUT"
