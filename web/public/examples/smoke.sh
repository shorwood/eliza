#!/usr/bin/env bash
set +x
set -euo pipefail
: "${ELIZA_API_ROOT:?Set ELIZA_API_ROOT to an existing instance}"
api_root=${ELIZA_API_ROOT%/}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
umask 077
printf 'Content-Type: application/json\n' > "$work/headers"
if [ -n "${ELIZA_API_KEY:-}" ]; then
  printf 'Authorization: Bearer %s\n' "$ELIZA_API_KEY" >> "$work/headers"
fi

curl --fail-with-body --silent --show-error --connect-timeout 5 --max-time 15 \
  --retry 2 --retry-max-time 45 -H "@$work/headers" \
  "$api_root/openai/v1/chat/completions" \
  -d '{"model":"eliza-1966","messages":[{"role":"user","content":"I am sad."}]}' \
  -o "$work/completion.json"
jq -e '.choices[0].message.content | type == "string" and length > 0' \
  "$work/completion.json" > /dev/null

# A partial stream is never retried or concatenated with another attempt.
curl --fail-with-body --silent --show-error --no-buffer --connect-timeout 5 --max-time 15 \
  -H "@$work/headers" "$api_root/openai/v1/chat/completions" \
  -d '{"model":"eliza-1966","stream":true,"messages":[{"role":"user","content":"I am sad."}]}' \
  -o "$work/stream.txt"
grep -qx 'data: \[DONE\]' "$work/stream.txt"

status=$(curl --silent --show-error --connect-timeout 5 --max-time 15 \
  -H "@$work/headers" "$api_root/openai/v1/chat/completions" \
  -d '{"model":"eliza-1966","messages":[]}' \
  -o "$work/error.json" --write-out '%{http_code}')
test "$status" = 400
jq -e '.error.message | type == "string" and length > 0' "$work/error.json" > /dev/null
printf 'ELIZA completion, stream, and validation error passed\n'
