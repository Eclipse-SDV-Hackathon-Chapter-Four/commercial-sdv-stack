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
# Convenience runner for the authorization integration tests.
#
#   - If a local `cargo` is available, just runs `cargo test`.
#   - Otherwise (this environment has no host toolchain) it builds the test
#     binary inside a Rust container and executes it on the host, where
#     `docker` and localhost:20002 are reachable.
#
# Extra args are passed through to libtest, e.g.:
#   ./run.sh --skip expired        # skip the slow ~70s expiry check
#   ./run.sh --nocapture           # show per-test output

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STACK_DIR="${STACK_DIR:-$(cd "$SCRIPT_DIR/.." && pwd)}"
RUST_IMAGE="${RUST_IMAGE:-rust:1-bookworm}"
CARGO_CACHE_VOL="${CARGO_CACHE_VOL:-sdv-cargo-cache}"

if command -v cargo >/dev/null 2>&1; then
  echo "[run.sh] Using local cargo."
  exec env STACK_DIR="$STACK_DIR" cargo test --manifest-path "$SCRIPT_DIR/Cargo.toml" -- "$@"
fi

echo "[run.sh] No local cargo; compiling test binary in ${RUST_IMAGE}..."
docker run --rm \
  -v "$SCRIPT_DIR":/work -w /work \
  -v "$CARGO_CACHE_VOL":/usr/local/cargo/registry \
  "$RUST_IMAGE" cargo test --no-run

# The container writes to the bind-mounted target/; pick the freshest test binary.
bin="$(ls -t "$SCRIPT_DIR"/target/debug/deps/authorization-* 2>/dev/null | grep -v '\.d$' | head -1 || true)"
if [ -z "$bin" ]; then
  echo "[run.sh] could not locate the compiled test binary under target/debug/deps/" >&2
  exit 1
fi

echo "[run.sh] Running $(basename "$bin") on host (STACK_DIR=$STACK_DIR)"
exec env STACK_DIR="$STACK_DIR" "$bin" "$@"
