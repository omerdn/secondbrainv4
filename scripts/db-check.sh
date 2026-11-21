#!/usr/bin/env sh
# Verifies the local dev database (T0.1.3): reachable, PostgreSQL 17, required extensions present
# in the app database and in template1, and both extensions working. Exits non-zero on failure.
set -eu

user="${POSTGRES_USER:-secondbrain}"
db="${POSTGRES_DB:-secondbrain}"

q() {
  docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -U "$user" -d "$1" -tAc "$2"
}

fail() {
  echo "db-check: FAIL - $1" >&2
  exit 1
}

major="$(q "$db" "SELECT current_setting('server_version_num')::int / 10000")"
[ "$major" = "17" ] || fail "expected PostgreSQL 17, got major version '$major'"

for target in "$db" template1; do
  for ext in pg_trgm unaccent; do
    found="$(q "$target" "SELECT count(*) FROM pg_extension WHERE extname = '$ext'")"
    [ "$found" = "1" ] || fail "extension '$ext' missing in database '$target'"
  done
done

# Smoke-test that the extensions actually work on Turkish text.
[ "$(q "$db" "SELECT unaccent('Çalışma Öğün')")" = "Calisma Ogun" ] || fail "unaccent smoke test"
[ "$(q "$db" "SELECT similarity('rutin', 'rutinler') > 0.3")" = "t" ] || fail "pg_trgm smoke test"

echo "db-check: OK (PostgreSQL 17, pg_trgm + unaccent in '$db' and template1)"
