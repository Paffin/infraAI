#!/usr/bin/env bash
# End-to-end smoke test of a running stack:  deploy/smoke.sh http://localhost:8080 admin@example.com 'password'
# Signs in, loads the editor bootstrap, creates a project from a reference architecture, evaluates it,
# asks what to improve first, simulates the loss of a site (with the steps of the recovery), reads the release tables and previews a placement. Exits non-zero on the first failed expectation.
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

# a reference architecture that is meant to survive the loss of a site (not the single-data-center one, which survives a rack)
WANT=${SMOKE_TEMPLATE:-two-dc-witness}
TEMPLATE=$(echo "$BOOT" | json "next((t['id'] for t in d['templates'] if t['id'] == '$WANT'), d['templates'][0]['id'])")
PID=$(api -d "{\"name\":\"smoke\",\"templateId\":\"$TEMPLATE\"}" "$BASE/api/projects" | json "d['id']")
ARCH=$(api "$BASE/api/projects/$PID" | json "json.dumps(d['architecture'])")
SCORE=$(echo "{\"architecture\":$ARCH}" | api -d @- "$BASE/api/evaluate" | json "d['score']['value']")
echo "reference architecture '$TEMPLATE' scores $SCORE"
python3 -c "import sys; sys.exit(0 if float('$SCORE') >= 80 else 1)" || fail "reference architecture scored $SCORE"

# the expected availability comes with the report: a number between a nine and all of the year, against the target the template carries
AVAIL=$(echo "{\"architecture\":$ARCH}" | api -d @- "$BASE/api/evaluate" | json "'%.5f' % d['availability']['estimate']")
echo "expected availability $AVAIL"
python3 -c "import sys; sys.exit(0 if 0.99 < float('$AVAIL') < 1 else 1)" || fail "the availability estimate is $AVAIL"

# what to improve first: with the entry point cut down to one instance the engine finds, tries and ranks the fix that closes it
WEAK=$(echo "$ARCH" | python3 -c "import json,sys; a=json.load(sys.stdin); [n['props'].__setitem__('replicas', 1) for n in a['nodes'] if n['id'] == 'gslb']; print(json.dumps(a))")
PLAN=$(echo "{\"architecture\":$WEAK}" | api -d @- "$BASE/api/recommendations")
FIRST=$(echo "$PLAN" | json "d['items'][0]['id'] if d['items'] else ''")
echo "first thing to improve: ${FIRST:-none}"
[ -n "$FIRST" ] || fail "no recommendation for a design with a single-instance entry point"
echo "$PLAN" | json "1 if all(i['effect']['critical']['after'] <= i['effect']['critical']['before'] for i in d['items']) else 0" | grep -q 1 || fail "a recommendation makes the design worse"

ZONE=$(echo "$ARCH" | json "d['zones'][0]['id']")
STATE=$(echo "{\"architecture\":$ARCH,\"scenario\":{\"type\":\"zone\",\"zoneId\":\"$ZONE\"}}" | api -d @- "$BASE/api/simulate" | json "d['state']")
echo "losing site $ZONE: service is $STATE"
[ "$STATE" != "down" ] || fail "the reference architecture does not survive the loss of a site"

# the story of a recovery: a failure that heals by itself comes with the steps it takes
TIMELINE=$(echo "{\"architecture\":$ARCH,\"scenario\":{\"type\":\"zone\",\"zoneId\":\"$ZONE\"}}" | api -d @- "$BASE/api/simulate" | json "len(d.get('timeline', []))")
echo "recovery timeline: $TIMELINE frames"

# product versions and the placement wizard
TABLES=$(api "$BASE/api/versions" | json "len(d['tables'])")
[ "$TABLES" -ge 50 ] || fail "only $TABLES release tables"
echo "release tables: $TABLES"
FIRST=$(api "$BASE/api/placement/options" | json "d['defaultPattern'] if d['defaultPattern'] in [p['id'] for p in d['patterns']] else ''")
[ -n "$FIRST" ] || fail "the wizard has no pattern to offer first"
echo "the wizard offers '$FIRST' first"
SITES=$(echo '{"pattern":"n-sites","sites":5}' | api -d @- "$BASE/api/placement/preview" | json "len(d['sites'])")
[ "$SITES" -ge 5 ] || fail "the placement preview for 5 sites has $SITES sites"
echo "placement for 5 sites: $SITES sites"

api -X DELETE "$BASE/api/projects/$PID" >/dev/null
curl -fsS "$BASE/healthz" >/dev/null || fail "healthz"
echo "OK"
