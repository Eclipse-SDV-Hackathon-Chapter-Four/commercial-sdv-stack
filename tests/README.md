<!--
SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Apache License Version 2.0 which is available at
https://www.apache.org/licenses/LICENSE-2.0

SPDX-License-Identifier: Apache-2.0
-->

# Integration Tests — Authorization Guarantees

Reproducible integration tests that prove the Commercial SDV Stack enforces its
security model: **valid attested workloads succeed; unauthorized workloads, forbidden
operations, bad audiences, and expired or forged credentials all fail closed.**

Addresses [issue #8](https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/OPHF-commercial-sdv-stack/issues/8).

## What is tested, and where

The stack has **two authorization enforcement points**. Both validate a JWT-SVID
against a required audience via the SPIRE Agent Workload API, then apply an
[Open Policy Agent](https://www.openpolicyagent.org/) (Rego) rule:

| Enforcement point | Surface | Required audience | Rego decision | Allowed |
| --- | --- | --- | --- | --- |
| `sovd-cda` | HTTP `:20002` (SOVD/REST) | `sovd.cda` | `service_name ∈ allowed_services[spiffe_id]` | `…/vehicle/powertrain-mode-controller` → `Powertrain_Mode_Read`, `Powertrain_Mode_Write` |
| `powertrain-mode-controller` | uProtocol RPC (MQTT) | `powertrain.mode-control` | `method_id ∈ allowed_method_ids[spiffe_id]` | `…/backend/fms` → methods `1`, `2` |

**This suite drives the CDA HTTP enforcement point.** It is the ideal test surface: it
exercises *both* the audience check and the Rego check, it is plain HTTP + Bearer token,
and every scenario can be produced deterministically by minting JWT-SVIDs from the
running `spire-server`. The uProtocol path uses the identical Rego pattern and is covered
by the demo flow (see [Not yet automated](#not-yet-automated)).

### How the matrix is produced

Tests mint JWT-SVIDs with `spire-server jwt mint`, which lets us forge any
`spiffe_id` / `audience` / `ttl` combination on demand. Minting is an **admin** operation
that bypasses workload attestation — which is exactly right here: we are validating the
CDA's **token-validation and authorization** layers, not node/workload attestation.
Attestation is covered separately (see [Not yet automated](#not-yet-automated)).

This is a standalone Cargo crate at the **commercial-sdv-stack root** (`tests/`), next
to `docker-compose.yaml`. It is deliberately **not** a member of the `uservices`
workspace, so it never pulls in the musl/cross build — it just drives the running stack.
The crate shells out to `docker compose exec spire-server ...` to mint tokens and uses
`reqwest` to call the CDA. Override the stack location with `STACK_DIR=...` if needed.

## Prerequisites

```bash
# From the commercial-sdv-stack root:

# Build images and start infra + the powertrain use case
docker compose --profile infra --profile powertrain up -d --build

# Register the workloads with SPIRE
scripts/register_workloads.sh
```

Needs a Rust toolchain (`cargo`) plus a running `docker` daemon. The suite reaches the
CDA at `http://localhost:20002`.

## Running

```bash
cd tests
cargo test                  # runs the whole matrix; exit code non-zero on any failure
cargo test -- --nocapture   # see per-test detail
```

No local Rust toolchain? Use the wrapper — it compiles the test binary in a Rust
container and runs it on the host (where `docker` and `localhost:20002` are reachable):

```bash
./run.sh                    # whole matrix
./run.sh --skip expired     # skip the slow ~70s expiry check
```

`cargo test` output is the pass/fail report (one line per `#[test]`, summarised at the
end) — suitable for CI and presentation. `spire-server` must be running for the suite to
mint tokens; the CDA must be up for the HTTP checks to pass.

> **Note:** `expired_token_is_denied` sleeps ~70s (past SPIRE's clock-skew leeway, see
> below). Skip the slow path during quick iteration with
> `cargo test -- --skip expired`.

### Configuration

Override via environment variables (see `src/lib.rs`): `STACK_DIR`, `CDA_BASE`,
`CDA_PORT`, `TRUST_DOMAIN`, `SPIFFE_PMC`, `SPIFFE_PROPERTIES`, `SPIFFE_UNKNOWN`,
`AUD_CDA`, `AUD_WRONG`, `PWT_DATA_PATH`, `EXPIRY_WAIT_SECONDS`.

## Expected results

| # | Scenario | Credential presented | Expected |
| --- | --- | --- | --- |
| 1 | Authorized read | `powertrain-mode-controller`, aud `sovd.cda` | **ALLOW** (2xx) |
| 2 | Authorized write | `powertrain-mode-controller`, aud `sovd.cda` | **ALLOW** (2xx) |
| 3 | Registered but forbidden identity | `vehicle/properties` (no allow-list entry) | **DENY** (403) |
| 4 | Unknown SPIFFE ID | `vehicle/intruder` (unregistered) | **DENY** (403) |
| 5 | Wrong JWT audience | correct id, aud `wrong.audience` | **DENY** (401) |
| 6 | Expired JWT | correct id, `ttl 2s`, wait `>60s` (leeway) | **DENY** (403) |
| 7 | Missing token | *(no Authorization header)* | **DENY** (401) |
| 8 | Tampered signature | valid token, middle sig byte flipped | **DENY** (403) |
| 9 | `alg:none` forged token | unsigned algorithm-confusion token | **DENY** (401) |
| 10 | Multi-audience token | aud `[sovd.cda, other.service]` | **ALLOW** (2xx) — audience is *contains*, not *equals* |

The suite classifies any `2xx` as **ALLOW** and any `401`/`403` as **DENY** (a `2xx` on a
deny row is a security hole and fails; a `5xx`/unreachable also fails). The exact
`401`-vs-`403` split above reflects the code paths (401 = authentication/token failure,
403 = Rego authorization denial); the pass/fail logic does not depend on which of the two
a deny returns.

### Observed behaviours (verified against the live stack)

- **JWT validation enforces the signature.** A token with a single flipped byte in the
  signature is rejected (`error in cryptographic primitive`). *Note for test authors:* in a
  64-byte ES256 signature the **last** base64url character carries only 2 significant bits,
  so tampering it is often a no-op — flip a character in the middle (the suite does).
- **`exp` is enforced with a ~60-second clock-skew leeway.** SPIRE's JWT validation
  (go-jose) accepts a token for up to ~60s past its `exp`. A token expired by less than
  that is still accepted; beyond it the agent reports `token has expired` and the CDA
  returns 403. The expiry test therefore waits `EXPIRY_WAIT_SECONDS` (default 70s). This
  answers issue #8's "how long existing JWT-SVIDs remain usable" for the near-expiry edge:
  **exp + ~60s**.

## Demo sequence (presentation)

```bash
# From the commercial-sdv-stack root:
docker compose --profile infra --profile powertrain up -d --build   # bring the stack up
scripts/register_workloads.sh                                        # register workloads
( cd tests && cargo test )                                           # green matrix on screen
```

The `cargo test` summary is the money shot: one `#[test]` per guarantee, all green,
non-zero exit on any regression.

## Not yet automated

These scenarios from issue #8 are **documented manual procedures** here; they mutate
container/stack state (stopping SPIRE, launching rogue containers) and are better run
deliberately than in the core matrix. Good candidates for a follow-up `cases/` expansion.

### Attestation — a modified image cannot obtain the expected identity

SPIRE attests workloads by **Docker image id *and* the `UP_LOCAL_ADDRESS` env selector**
(see `scripts/register_workloads.sh`). A container that is not the registered image — or
has the wrong env — gets no SVID:

```bash
# Launch a rogue container sharing the vehicle agent's Workload API socket:
docker run --rm -it \
  -v commercial-sdv-stack_spire-agent-vehicle-socket:/tmp/spire-agent/public \
  ghcr.io/spiffe/spire-agent:1.15.3 \
  api fetch jwt -audience sovd.cda \
  -socketPath /tmp/spire-agent/public/api.sock
# Expected: no SVID returned — the image id is not a registered selector.
```

### Offline behavior — backend SPIRE unavailable

```bash
# Mint a short-lived and a longer-lived token BEFORE going offline:
docker compose exec -T spire-server /opt/spire/bin/spire-server jwt mint \
  -socketPath /run/spire/server/private/api.sock \
  -spiffeID spiffe://sdv.eclipse.org/vehicle/powertrain-mode-controller \
  -audience sovd.cda -ttl 300s           # cached valid credential
# ...and one with -ttl 5s for the "expired" case.

docker compose stop spire-server          # backend SPIRE unavailable

# Cached valid credential -> still ALLOW: the agent validates against its cached JWT
# bundle (public keys) without the server.
curl -s -o /dev/null -w '%{http_code}\n' \
  -H "Authorization: Bearer <valid-token>" \
  http://localhost:20002/vehicle/v15/components/blueprint-ecu/data/powertrain_mode

# Expired credential -> DENY: fails securely even while offline.
# New SVID issuance is impossible while the server is down (JWT-SVIDs are fetched
# from the server on demand, not minted by the agent).

docker compose start spire-server         # connectivity returns -> issuance resumes
```

**Characterization notes to capture during the demo:**

- **JWT-SVID lifetime** — minted TTLs are capped by the server (observe the
  `JWT-SVID lifetime was capped…` notice); tokens are usable until their `exp`.
- **Server restart invalidates all JWTs** — `spire-server` uses an **in-memory**
  `KeyManager` (`config/spire/server/server.conf`), so restarting it rotates the JWT
  signing key and **all previously issued JWTs stop validating**. Agents use a **disk**
  `KeyManager` and recover their own identity without re-attestation. This is a real
  design decision worth surfacing: switch the server to a persistent `KeyManager` if
  tokens must survive a backend restart.

## Layout

```
commercial-sdv-stack/
├── docker-compose.yaml       # the stack this suite drives
└── tests/                    # standalone Cargo crate (not a uservices workspace member)
    ├── Cargo.toml
    ├── README.md             # this file
    ├── src/
    │   └── lib.rs            # config, SPIRE minting/forging, HTTP + assert helpers
    └── tests/
        └── authorization.rs  # the #[test] matrix
```
