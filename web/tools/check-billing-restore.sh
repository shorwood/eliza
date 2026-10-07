#!/usr/bin/env bash
set -euo pipefail
container="${1:?Pass the dedicated local PostgreSQL test container name}"
case "$container" in eliza-pr016-postgres) ;; *) echo 'Use the dedicated eliza-pr016-postgres container.' >&2; exit 1 ;; esac
backup="$(mktemp)"
trap 'rm -f "$backup"' EXIT
docker exec "$container" pg_dump -U postgres -d eliza_test --format=custom > "$backup"
docker exec "$container" psql -U postgres -d postgres -c 'DROP DATABASE IF EXISTS eliza_restore_test' >/dev/null
docker exec "$container" psql -U postgres -d postgres -c 'CREATE DATABASE eliza_restore_test' >/dev/null
docker exec -i "$container" pg_restore -U postgres -d eliza_restore_test < "$backup"
for table in accounts credentials subscriptions api_keys billing_events rate_limits; do
  original="$(docker exec "$container" psql -U postgres -d eliza_test -Atc "SELECT md5(coalesce(string_agg(row::text, ',' ORDER BY row::text), '')) FROM $table row")"
  restored="$(docker exec "$container" psql -U postgres -d eliza_restore_test -Atc "SELECT md5(coalesce(string_agg(row::text, ',' ORDER BY row::text), '')) FROM $table row")"
  test "$original" = "$restored"
done
docker exec "$container" psql -U postgres -d postgres -c 'DROP DATABASE eliza_restore_test' >/dev/null
echo 'All six billing tables restored with identical rows.'
