#!/bin/sh

set -eu

example_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repository_dir="$(CDPATH= cd -- "$example_dir/../.." && pwd)"
compose_file="$example_dir/compose.yaml"

cleanup() {
    docker compose --file "$compose_file" down --remove-orphans
}

image=$(nix build --no-link --print-out-paths "$repository_dir#dockerImage")
docker load --input "$image"

trap cleanup EXIT

echo "Open WebUI: http://127.0.0.1:${ELIZA_OPEN_WEBUI_PORT:-3000}"
docker compose --file "$compose_file" up --remove-orphans
