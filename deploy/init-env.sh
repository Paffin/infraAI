#!/bin/sh
# Writes .env from .env.example with the secrets generated: APP_SECRET, POSTGRES_PASSWORD and the password of the first administrator.
# It does nothing when .env exists, so it can be run again (and `make .env` is the same thing); it never changes a file that is there.
#
#   sh deploy/init-env.sh [directory]      # the directory with .env.example; default: the root of the repository
#
# A secret that is a string from a published template is not a secret: the server refuses to start in production with the placeholder of .env.example
# (and compose refuses an empty one), so a hand-copied file fails at the start with a message instead of running with a key everybody knows.
set -eu
umask 077

root=$(cd "${1:-$(dirname "$0")/..}" && pwd)
example="$root/.env.example"
target="$root/.env"

fail() { echo "init-env: $*" >&2; exit 1; }

[ -f "$example" ] || fail "$example not found"
if [ -e "$target" ]; then
  echo ".env exists, left as it is: $target"
  exit 0
fi

# $1 random bytes as 2*$1 hex characters: openssl when there is one, the kernel's source otherwise
random_hex() {
  if command -v openssl >/dev/null 2>&1; then
    out=$(openssl rand -hex "$1" 2>/dev/null) || out=
  else
    out=
  fi
  if [ "${#out}" -ne $(($1 * 2)) ]; then
    out=$(head -c "$1" /dev/urandom | od -An -tx1 | tr -d ' \n')
  fi
  [ "${#out}" -eq $(($1 * 2)) ] || fail "could not read $1 random bytes"
  printf '%s' "$out"
}

secret=$(random_hex 32)      # 64 characters
database=$(random_hex 16)    # 32 characters; hex, so it is safe inside DATABASE_URL
admin=$(random_hex 12)       # 24 characters; the first start creates the administrator with it

tmp="$target.tmp.$$"
trap 'rm -f "$tmp"' EXIT
sed -e "s|^APP_SECRET=.*|APP_SECRET=$secret|" \
    -e "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=$database|" \
    -e "s|^ADMIN_PASSWORD=.*|ADMIN_PASSWORD=$admin|" "$example" > "$tmp"
# the substitutions must have happened: an example that lost one of these lines would give a file that fails later and elsewhere
for pair in "APP_SECRET=$secret" "POSTGRES_PASSWORD=$database" "ADMIN_PASSWORD=$admin"; do
  grep -q "^$pair\$" "$tmp" || fail "${pair%%=*} is not a line of $example"
done
mv "$tmp" "$target"
trap - EXIT

email=$(sed -n 's/^ADMIN_EMAIL=//p' "$target" | head -n 1)
echo "created $target (readable by you only)"
echo "  APP_SECRET and POSTGRES_PASSWORD are generated: keep the file, losing APP_SECRET signs everybody out and makes the stored secrets unreadable"
echo "  sign in as ${email:-admin@example.com} with the ADMIN_PASSWORD from this file (used once, at the first start; change it in the application)"
