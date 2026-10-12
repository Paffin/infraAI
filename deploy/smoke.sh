#!/usr/bin/env bash
# End-to-end smoke test of a running stack:  deploy/smoke.sh http://localhost:8080 admin@example.com 'password'
# Signs in, loads the editor bootstrap, creates a project from a reference architecture, evaluates it and
# simulates the loss of a site. Exits non-zero on the first failed expectation.
set -euo pipefail
BASE=${1:-http://localhost:8080}
EMAIL=${2:-admin@example.com}
PASSWORD=${3:?usage: smoke.sh BASE EMAIL PASSWORD}
JAR=$(mktemp)
trap 'rm -f "$JAR"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
api() { curl -fsS -b "$JAR" -c "$JAR" -H 'Content-Type: application/json' "$@"; }

for i in $(seq 1 60); do
  if curl -fsS "$BASE/readyz" >/dev/null 2>&1; then break; fi
  [ "$i" = 60 ] && fail "service did not become ready"
  sleep 2
done
echo "ready"

curl -fsS "$BASE/" | grep -qi '<div id="app">' || fail "the single-page app is not served"
echo "frontend served"

api -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" "$BASE/api/auth/login" >/dev/null || fail "login"
echo "signed in"

BOOT=$(api "$BASE/api/bootstrap")
kinds=$(echo "$BOOT" | json "len(d['catalog']['kinds'])")
rules=$(echo "$BOOT" | json "len(d['rules'])")
[ "$kinds" -ge 20 ] || fail "catalog has $kinds kinds"
[ "$rules" -ge 40 ] || fail "only $rules rules"
echo "bootstrap: $kinds kinds, $rules rules"

TEMPLATE=$(echo "$BOOT" | json "d['templates'][0]['id']")
PID=$(api -d "{\"name\":\"smoke\",\"templateId\":\"$TEMPLATE\"}" "$BASE/api/projects" | json "d['id']")
ARCH=$(api "$BASE/api/projects/$PID" | json "json.dumps(d['architecture'])")
SCORE=$(echo "{\"architecture\":$ARCH}" | api -d @- "$BASE/api/evaluate" | json "d['score']['value']")
echo "reference architecture '$TEMPLATE' scores $SCORE"
python3 -c "import sys; sys.exit(0 if float('$SCORE') >= 80 else 1)" || fail "reference architecture scored $SCORE"

ZONE=$(echo "$ARCH" | json "d['zones'][0]['id']")
STATE=$(echo "{\"architecture\":$ARCH,\"scenario\":{\"type\":\"zone\",\"zoneId\":\"$ZONE\"}}" | api -d @- "$BASE/api/simulate" | json "d['state']")
echo "losing site $ZONE: service is $STATE"
[ "$STATE" != "down" ] || fail "the reference architecture does not survive the loss of a site"

api -X DELETE "$BASE/api/projects/$PID" >/dev/null
curl -fsS "$BASE/healthz" >/dev/null || fail "healthz"
echo "OK"
