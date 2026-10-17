#!/bin/sh
# Checks that the quick start holds together: deploy/init-env.sh writes a .env that docker compose accepts, .env.example alone is refused with a message that
# says what to do, and deploy/compose.try.yml starts the published image with the hardening of docker-compose.yml. Run by CI (the `docs` job); run it
# yourself after touching any of these files:   sh deploy/check-env.sh
# Without Docker the compose checks are skipped with a notice (in CI, where `CI` is set, a missing Docker is a failure).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() { echo "check-env: FAIL: $*" >&2; exit 1; }
ok() { echo "check-env: ok: $*"; }
value() { sed -n "s/^$2=//p" "$1" | head -n 1; }
fresh() { mkdir -p "$work/$1" && cp "$root/.env.example" "$work/$1/.env.example"; }

# --- the example ships no usable secret
for key in APP_SECRET POSTGRES_PASSWORD; do
  [ -z "$(value "$root/.env.example" $key)" ] || fail "$key in .env.example is not empty: a copy of the example would run with a published value"
done
ok ".env.example has empty APP_SECRET and POSTGRES_PASSWORD"

# --- init-env.sh
fresh a
sh "$root/deploy/init-env.sh" "$work/a" >/dev/null
env_a="$work/a/.env"
printf '%s\n' "$(value "$env_a" APP_SECRET)" | grep -Eq '^[0-9a-f]{64}$' || fail "APP_SECRET is not 64 hexadecimal characters: $(value "$env_a" APP_SECRET)"
printf '%s\n' "$(value "$env_a" POSTGRES_PASSWORD)" | grep -Eq '^[0-9a-f]{32}$' || fail "POSTGRES_PASSWORD is not 32 hexadecimal characters"
printf '%s\n' "$(value "$env_a" ADMIN_PASSWORD)" | grep -Eq '^[0-9a-f]{24}$' || fail "ADMIN_PASSWORD is not 24 hexadecimal characters"
[ "$(ls -ld "$env_a" | cut -c1-10)" = "-rw-------" ] || fail ".env is readable by others: $(ls -ld "$env_a" | cut -c1-10)"
case "$(value "$env_a" APP_SECRET)$(value "$env_a" POSTGRES_PASSWORD)" in *change-me*|*CHANGE_ME*) fail "a placeholder was written";; esac
ok "a new .env has a 64-hex APP_SECRET, a 32-hex POSTGRES_PASSWORD, an ADMIN_PASSWORD, and mode 600"

cp "$env_a" "$work/a.env.first"
sh "$root/deploy/init-env.sh" "$work/a" >/dev/null
cmp -s "$env_a" "$work/a.env.first" || fail "a second run changed .env"
ok "a second run leaves .env byte for byte as it was"

fresh b
sh "$root/deploy/init-env.sh" "$work/b" >/dev/null
[ "$(value "$env_a" APP_SECRET)" != "$(value "$work/b/.env" APP_SECRET)" ] || fail "two installations got the same APP_SECRET"
ok "two installations get different secrets"

# without openssl: the kernel's source of random bytes. PATH holds only the tools the script uses, found where they are, and not openssl.
bin="$work/bin"
mkdir -p "$bin"
for tool in sh sed head od tr grep mv rm dirname cat; do
  path=$(command -v "$tool") || fail "$tool is not on this machine"
  ln -s "$path" "$bin/$tool"
done
fresh c
PATH="$bin" "$bin/sh" "$root/deploy/init-env.sh" "$work/c" >/dev/null || fail "init-env.sh does not work without openssl"
printf '%s\n' "$(value "$work/c/.env" APP_SECRET)" | grep -Eq '^[0-9a-f]{64}$' || fail "without openssl APP_SECRET is not 64 hexadecimal characters"
ok "without openssl it reads /dev/urandom"

if command -v make >/dev/null 2>&1; then
  fresh d
  cp "$root/Makefile" "$work/d/Makefile"
  mkdir -p "$work/d/deploy"
  cp "$root/deploy/init-env.sh" "$work/d/deploy/init-env.sh"
  make -n -C "$work/d" .env | grep -q 'sh deploy/init-env.sh' || fail "make .env does not run deploy/init-env.sh"
  make -s -C "$work/d" .env | grep -q "$work/d/.env" || fail "make .env does not say where the file is"
  ok "make .env runs init-env.sh and says where the file is"
fi

# --- compose
if ! docker compose version >/dev/null 2>&1; then
  [ -z "${CI:-}" ] || fail "docker compose is not available in CI"
  echo "check-env: skipped: docker compose is not available, the compose checks were not run"
  exit 0
fi
compose() { dir=$1; shift; env -u APP_SECRET -u POSTGRES_PASSWORD -u IA_IMAGE -u TRUST_PROXY -u TRUST_PROXY_HOPS docker compose --project-directory "$dir" -f "$root/docker-compose.yml" "$@"; }

fresh e
cp "$root/.env.example" "$work/e/.env"
if compose "$work/e" config >/dev/null 2>"$work/e/err"; then fail "docker compose accepts the untouched .env.example"; fi
grep -q 'set APP_SECRET in .env' "$work/e/err" || fail "the refusal does not say what to do: $(cat "$work/e/err")"
ok "docker compose refuses the untouched .env.example and says to set APP_SECRET"

compose "$work/a" config >/dev/null || fail "docker compose refuses the generated .env"
ok "docker compose accepts the generated .env"

env -u APP_SECRET -u POSTGRES_PASSWORD IA_IMAGE=infra-architect:ci docker compose --project-directory "$work/a" -f "$root/docker-compose.yml" -f "$root/deploy/compose.try.yml" config --format json |
  python3 -c '
import json, sys
app = json.load(sys.stdin)["services"]["app"]
problems = []
if "build" in app: problems.append("it still builds")
if app["image"] != "infra-architect:ci": problems.append("image is %s, not IA_IMAGE" % app["image"])
if app.get("read_only") is not True: problems.append("the root file system is not read only")
if app.get("cap_drop") != ["ALL"]: problems.append("capabilities are not dropped")
if "no-new-privileges:true" not in app.get("security_opt", []): problems.append("no-new-privileges is missing")
if problems: sys.exit("; ".join(problems))
' || fail "deploy/compose.try.yml: see above"
ok "compose.try.yml: no build, the image is IA_IMAGE, read-only root, no capabilities, no-new-privileges"
echo "check-env: all checks passed"
