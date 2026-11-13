#!/usr/bin/env bash
# Runs buildDebian.sh in a Debian amd64 container, so any Linux host with podman builds the
# same assets (x86_64 natively, arm64 through Rosetta or qemu binfmt).
set -euo pipefail

ROOT_FOLDER="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="rustitles-build"

if ! command -v podman >/dev/null 2>&1; then
    echo "Error: podman is required." >&2
    exit 1
fi

podman build --platform linux/amd64 --tag "$IMAGE" \
    --file "$ROOT_FOLDER/.scripts/linuxBuild.Containerfile" "$ROOT_FOLDER/.scripts"

# The registry volume keeps downloaded crates between runs; target/ lives in the mounted repo.
exec podman run --rm --platform linux/amd64 \
    --volume "$ROOT_FOLDER:/src" \
    --volume rustitles-cargo-registry:/opt/cargo/registry \
    --workdir /src "$IMAGE" bash .scripts/buildDebian.sh
