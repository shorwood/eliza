#!/bin/sh

set -eu

port="${ELIZA_EXAMPLE_PORT:-8787}"
base_url="http://127.0.0.1:${port}"

target/debug/eliza serve --bind "127.0.0.1:${port}" &
server_pid=$!

cleanup() {
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
}

trap cleanup EXIT HUP INT TERM

attempt=0
until curl --fail --silent "$base_url/healthz" >/dev/null; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        wait "$server_pid" || status=$?
        exit "${status:-1}"
    fi

    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        echo "ELIZA did not become ready at $base_url" >&2
        exit 1
    fi
    sleep 0.05
done

ELIZA_BASE_URL="$base_url" "$@"
