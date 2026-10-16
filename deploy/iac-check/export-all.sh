#!/bin/sh
# Writes every IaC format of every reference architecture (and the demo project) into $OUT.
# Runs inside the Rust build container:  deploy/iac-check/export-all.sh /out
set -eu
OUT=${1:-/out}
rm -rf "$OUT"/*
cd /src
cargo build -q -p engine --example export
BIN=${CARGO_TARGET_DIR:-target}/debug/examples/export
# every reference architecture there is (a file in defaults/templates is one), and the demo project
for tpl in demo $(cd crates/engine/defaults/templates && ls *.json | sed 's/\.json$//'); do
  for fmt in kubernetes helm terraform; do
    "$BIN" "$tpl" "$fmt" "$OUT/$tpl/$fmt" en >/dev/null 2>"$OUT/$tpl-$fmt.report.json"
  done
done
echo "exported to $OUT"
