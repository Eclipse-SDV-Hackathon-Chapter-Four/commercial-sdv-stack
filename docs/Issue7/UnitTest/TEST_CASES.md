# Issue 7 – Test Cases: CDA SOVD API via Protected Unix Domain Socket

This document is AI generated.

| | |
|---|---|
| **Script** | [run_tests.sh](run_tests.sh) |
| **Design** | [../design/cda-unix-socket.md](../design/cda-unix-socket.md) |
| **Branch** | `feature/issue7-cda-unix-socket-pmc-client` |
| **Last run** | 2026-10-06, Docker Desktop 28.1.1 (arm64) – 16 passed, 0 failed, 1 known limitation |

These are black-box integration tests against the running Docker Compose stack. They verify the transport
exposure, the socket protection, the container hardening, the end-to-end flow and the JWT-SVID/Rego
authorization.

## How to Run

```sh
docker compose --profile infra --profile powertrain up -d
scripts/register_workloads.sh
docs/Issue7/UnitTest/run_tests.sh
```

The script exits with `0` if no test failed. Known limitations are reported as `KNOWN` and do not fail the run.

Optional environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `COMPOSE_PROJECT_NAME` | `commercial-sdv-stack` | Prefix of the volume and network names |
| `TEST_CLIENT_IMAGE` | stack's `ecu-sim` image | Throwaway client image providing `sh` and `curl` |
| `CDA_PORT` | `20002` | Former CDA host port that must no longer be reachable |

Test clients are throwaway containers. TC14–TC17 temporarily register the SPIFFE ID
`spiffe://sdv.eclipse.org/vehicle/test-unauthorized` (selector `docker:env:SOVD_TEST_WORKLOAD=unauthorized`)
and delete it again when the script exits.

## Test Cases

### Transport exposure

| ID | Test case | Steps | Expected | Result |
|---|---|---|---|---|
| TC01 | CDA has no TCP listener | `docker compose exec sovd-cda ss -ltnH` | No listener on `:20002` | ✅ PASS |
| TC02 | Host cannot reach CDA | `curl http://localhost:20002/` on the host | Connection fails | ✅ PASS |
| TC03 | `vehicle-sovd` network cannot reach CDA over TCP | Container on `vehicle-sovd`: `curl http://sovd-cda:20002/` | curl exit `7` (connection refused) | ✅ PASS |
| TC04 | PMC's network has no route to CDA | Container on `vehicle-uprotocol`: `curl http://sovd-cda:20002/` | curl exit `6` (cannot resolve) | ✅ PASS |

### Socket protection

| ID | Test case | Steps | Expected | Result |
|---|---|---|---|---|
| TC05 | CDA user and socket permissions | `id`, `stat /run/cda /run/cda/cda.sock` in `sovd-cda` | `10001:10100`; dir `drwxr-x---`, socket `srwxrwx---`, both `10001 10100` | ✅ PASS |
| TC06 | Wrong group cannot connect | Container `--user 10003:10003` with socket volume, `curl --unix-socket` | curl exit `7` | ✅ PASS |
| TC07 | Missing token is rejected | Container `--user 10003:10100`, `GET …/data/powertrain_mode` without `Authorization` | HTTP `401` | ✅ PASS |
| TC08 | Socket cannot be deleted by PMC user | Container `--user 10002:10100` with read-only mount, `rm /run/cda/cda.sock` | `Read-only file system` | ✅ PASS |
| TC09 | Socket invisible without the volume | Container without the volume, `test -e /run/cda/cda.sock` | Not found | ✅ PASS |

### Container hardening

| ID | Test case | Steps | Expected | Result |
|---|---|---|---|---|
| TC10 | `sovd-cda` hardening | `docker inspect` ports, `CapDrop`, `SecurityOpt` | No published ports, `ALL`, `no-new-privileges` | ✅ PASS |
| TC11 | PMC hardening and network isolation | `docker inspect` user, networks, `CapDrop`, `SecurityOpt` | `10002:10100`, not on `vehicle-sovd`, `ALL`, `no-new-privileges` | ✅ PASS |

### End-to-end

| ID | Test case | Steps | Expected | Result |
|---|---|---|---|---|
| TC12 | PMC uses the Unix socket | PMC startup log | `Unix socket: /run/cda/cda.sock` | ✅ PASS |
| TC13 | Mode switching FMS → PMC → CDA → ECU | FMS and ecu-sim logs over 15 s | ≥ 2 `Successfully set powertrain mode` and ≥ 2 `2E 4E 66` requests (lock acquired/released per write) | ✅ PASS |

### Authorization (valid JWT-SVID, unauthorized SPIFFE ID)

| ID | Test case | Steps | Expected | Result |
|---|---|---|---|---|
| TC14 | Obtain test JWT-SVID | Temporary SPIRE entry, `spire-agent api fetch jwt -audience sovd.cda` | Token issued | ✅ PASS |
| TC15 | Read denied by Rego | `GET …/data/powertrain_mode` with test token | HTTP `403` | ✅ PASS |
| TC16 | Write rejected | `PUT …/data/powertrain_mode` with test token | HTTP `403` or `409` (lock check precedes Rego) | ✅ PASS (`409`) |
| TC17 | Lock acquisition rejected | `POST …/components/blueprint-ecu/locks` with test token | Rejected | ⚠️ KNOWN (`201`) |

## Known Limitation (TC17)

CDA's lock endpoints only check authentication (valid JWT-SVID), not the Rego policy. A workload that can reach
the socket and holds a valid JWT-SVID for `sovd.cda` can therefore acquire an ECU lock and block the PMC's
writes. Exposure is limited by the socket protection (TC05–TC09). Follow-up: implement CDA's vendor lock
validation hook in the SPIFFE security plugin and evaluate it against Rego; TC17 will then report `PASS`.

## Run Output (2026-10-06)

```text
== Issue 7 tests (project: commercial-sdv-stack)
PASS   TC01   CDA has no TCP listener on port 20002
PASS   TC02   Host cannot reach CDA on localhost:20002
PASS   TC03   vehicle-sovd network: sovd-cda:20002 refused
PASS   TC04   vehicle-uprotocol network (PMC's): sovd-cda not resolvable
PASS   TC05   CDA runs as 10001:10100, dir 0750 and socket 0770 owned 10001:10100
PASS   TC06   Client with volume but wrong group cannot connect
PASS   TC07   Client in group 10100 without token gets 401
PASS   TC08   Read-only mount prevents deleting the socket (PMC uid)
PASS   TC09   Container without the volume cannot see the socket
PASS   TC10   sovd-cda: no published ports, cap_drop ALL, no-new-privileges
PASS   TC11   PMC: user 10002:10100, not on vehicle-sovd, cap_drop ALL, no-new-privileges
PASS   TC12   PMC is configured to use the Unix socket
       (collecting logs for 12s)
PASS   TC13   End-to-end mode switching FMS -> PMC -> CDA (UDS + lock) -> ECU
PASS   TC14   Obtain JWT-SVID for unauthorized test identity
PASS   TC15   Unauthorized SPIFFE ID reading powertrain_mode is denied (403)
PASS   TC16   Unauthorized SPIFFE ID writing powertrain_mode is rejected (403/409)
KNOWN  TC17   Unauthorized SPIFFE ID acquiring an ECU lock is rejected -> HTTP 201 - lock endpoints are not covered by Rego (known limitation)

== Summary: 16 passed, 0 failed, 1 known limitation(s)
```
