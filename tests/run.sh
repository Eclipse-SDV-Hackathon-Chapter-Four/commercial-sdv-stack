#!/usr/bin/env bash
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Apache License Version 2.0 which is available at
# https://www.apache.org/licenses/LICENSE-2.0
#
# SPDX-License-Identifier: Apache-2.0
#
# AI assistance: parts of this file were generated with Claude Code (Opus 4.8)
# and reviewed and verified by the human contributor. All content is
# contributed under the Apache-2.0 license declared above.
#
# Runner for the authorization integration tests.
#
# The CDA is socket-hardened: its SOVD API is exposed ONLY over a Unix domain
# socket in the `cda-sovd-socket` volume (no TCP listener), reachable only by
# the `sovd-clients` group (GID 10100). So, unlike a TCP endpoint, the tests
# cannot be driven from the host. Instead we build one image (Rust + Docker CLI)
# and, in it:
#   1. compile the test binaries, then
#   2. run each binary in a container that mounts the CDA socket volume, joins
#      the sovd-clients group, and mounts the Docker socket (the tests mint
#      JWT-SVIDs and drive the stack via `docker compose`).
# Compiling and running in the same image avoids a glibc-version mismatch.
#
# Extra args are passed through to libtest, e.g.:
#   ./run.sh --include-ignored --test-threads=1   # full matrix incl. offline
#   ./run.sh --nocapture                          # show per-test output

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STACK_DIR="${STACK_DIR:-$(cd "$SCRIPT_DIR/.." && pwd)}"
RUNNER_IMAGE="${RUNNER_IMAGE:-sdv-authz-test-runner}"
CARGO_CACHE_VOL="${CARGO_CACHE_VOL:-sdv-cargo-cache}"

# Compose project of the running stack (its volumes/containers are name-scoped by
# it); defaults to the stack directory name, matching `docker compose`.
PROJECT="${COMPOSE_PROJECT_NAME:-$(basename "$STACK_DIR")}"
# The CDA SOVD socket volume and the group permitted to use the socket.
SOVD_SOCKET_VOLUME="${SOVD_SOCKET_VOLUME:-${PROJECT}_cda-sovd-socket}"
SOVD_CLIENTS_GID="${SOVD_CLIENTS_GID:-10100}"

echo "[run.sh] Building test-runner image (${RUNNER_IMAGE})..."
docker build -q -t "$RUNNER_IMAGE" -f "$SCRIPT_DIR/Dockerfile.runner" "$SCRIPT_DIR" >/dev/null

echo "[run.sh] Compiling test binaries..."
docker run --rm \
  -v "$SCRIPT_DIR":/work -w /work \
  -v "$CARGO_CACHE_VOL":/usr/local/cargo/registry \
  "$RUNNER_IMAGE" cargo test --no-run

# Pass STACK_DIR through unchanged (bind-mounted at the same path below) so the
# tests' `docker compose` calls resolve the compose file and the running project.
docker_envs=(
  -e "STACK_DIR=$STACK_DIR"
  -e "COMPOSE_PROJECT_NAME=$PROJECT"
  -e "CDA_SOCKET=${CDA_SOCKET:-/run/cda/cda.sock}"
)
# Forward any test-tuning overrides that happen to be set in the environment.
for v in CDA_BASE EXPIRY_WAIT_SECONDS READINESS_POLL_SECONDS TRUST_DOMAIN \
         SPIFFE_PMC SPIFFE_PROPERTIES SPIFFE_UNKNOWN AUD_CDA AUD_WRONG \
         PWT_DATA_PATH PWT_WRITE_BODY; do
  [ -n "${!v:-}" ] && docker_envs+=(-e "$v=${!v}")
done

# Run each compiled binary (newest build of each) in the socket-mounted
# container. Default run executes only `authorization` (offline's tests are
# #[ignore]d); pass `--include-ignored --test-threads=1` for the stateful set.
# --user 0:GID keeps file access (root) while presenting the sovd-clients group.
rc=0
found=
for name in authorization offline; do
  bin="$(ls -t "$SCRIPT_DIR"/target/debug/deps/${name}-* 2>/dev/null | grep -v '\.d$' | head -1 || true)"
  [ -n "$bin" ] || continue
  found=1
  rel="${bin#"$SCRIPT_DIR"/}"
  echo "[run.sh] Running ${name} (project=$PROJECT, socket volume=$SOVD_SOCKET_VOLUME)"
  docker run --rm \
    --user "0:${SOVD_CLIENTS_GID}" \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v "$STACK_DIR":"$STACK_DIR" \
    -v "$SCRIPT_DIR":/work \
    -v "$SOVD_SOCKET_VOLUME":/run/cda \
    -w "$STACK_DIR" \
    "${docker_envs[@]}" \
    "$RUNNER_IMAGE" "/work/$rel" "$@" || rc=1
done
if [ -z "$found" ]; then
  echo "[run.sh] could not locate any compiled test binary under target/debug/deps/" >&2
  exit 1
fi
exit "$rc"
