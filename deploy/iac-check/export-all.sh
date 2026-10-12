#!/bin/sh
# Writes every IaC format of every reference architecture (and the demo project) into $OUT.
# Runs inside the Rust build container:  deploy/iac-check/export-all.sh /out
set -eu
OUT=${1:-/out}
rm -rf "$OUT"/*
cd /src
cargo build -q -p engine --example export
BIN=${CARGO_TARGET_DIR:-target}/debug/examples/export
for tpl in demo two-dc-witness three-az-region multi-region-pilot-light event-driven-kafka; do
  for fmt in kubernetes helm terraform; do
    "$BIN" "$tpl" "$fmt" "$OUT/$tpl/$fmt" en >/dev/null 2>"$OUT/$tpl-$fmt.report.json"
  done
done
echo "exported to $OUT"
