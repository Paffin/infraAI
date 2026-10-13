#!/usr/bin/env bash
# Record a frontend test fixture of a simulation from the REAL engine:
#
#   deploy/record-sim.sh <template-id | path/to/architecture.json> '<scenario json>' <fixture-name>
#
# Writes frontend/tests/fixtures/sim-<fixture-name>.json = {"architecture": …, "scenario": …, "result": …}, where `result` is what
# `engine::sim::simulate_timeline` returns (shipped catalog and settings) – the playback tests run on it.
set -euo pipefail
SRC=${1:?usage: record-sim.sh <template-id | architecture.json> '<scenario json>' <fixture-name>}
SCENARIO=${2:?usage: record-sim.sh <template-id | architecture.json> '<scenario json>' <fixture-name>}
NAME=${3:?usage: record-sim.sh <template-id | architecture.json> '<scenario json>' <fixture-name>}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/frontend/tests/fixtures/sim-$NAME.json"
mkdir -p "$ROOT/frontend/tests/fixtures"
(cd "$ROOT/backend" && cargo run -q -p engine --example simulate -- "$SRC" "$SCENARIO" en) > "$OUT.result"
python3 - "$SRC" "$SCENARIO" "$OUT" "$ROOT" <<'PY'
import json, os, sys
src, scenario, out, root = sys.argv[1:5]
if os.path.exists(src):
    arch = json.load(open(src))
else:
    arch = json.load(open(f"{root}/backend/crates/engine/defaults/templates/{src}.json"))["architecture"]
result = json.load(open(out + ".result"))
json.dump({"architecture": arch, "scenario": json.loads(scenario), "result": result}, open(out, "w"), ensure_ascii=False, indent=1)
os.remove(out + ".result")
PY
echo "recorded $OUT"
