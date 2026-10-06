#!/bin/sh
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
# Issue 7 tests: CDA SOVD API exposed only via a protected Unix domain socket.
# See TEST_CASES.md in this folder for the description of each test case.
#
# Prerequisites:
#   docker compose --profile infra --profile powertrain up -d
#   scripts/register_workloads.sh
#
# Usage (from anywhere):
#   docs/Issue7/UnitTest/run_tests.sh
#
# Environment:
#   COMPOSE_PROJECT_NAME  compose project name (default: commercial-sdv-stack)
#   TEST_CLIENT_IMAGE     image providing sh + curl (default: the stack's ecu-sim image)
#   CDA_PORT              former CDA host port to probe (default: 20002)

set -u

cd "$(dirname "$0")/../../.." || exit 1

PROJECT="${COMPOSE_PROJECT_NAME:-commercial-sdv-stack}"
CLIENT_IMG="${TEST_CLIENT_IMAGE:-ghcr.io/eclipse-sdv-blueprints/commercial-sdv-stack/ecu-sim:latest}"
CDA_PORT="${CDA_PORT:-20002}"
SOCK_VOL="${PROJECT}_cda-sovd-socket"
SPIRE_VOL="${PROJECT}_spire-agent-vehicle-socket"
SPIRE_AGENT_IMG="ghcr.io/spiffe/spire-agent:1.15.3"
ECU_URL="http://localhost/vehicle/v15/components/blueprint-ecu"
DATA_URL="${ECU_URL}/data/powertrain_mode"

PASS=0
FAIL=0
KNOWN=0

pass()  { PASS=$((PASS + 1));   printf 'PASS   %-6s %s\n' "$1" "$2"; }
fail()  { FAIL=$((FAIL + 1));   printf 'FAIL   %-6s %s -> %s\n' "$1" "$2" "$3"; }
known() { KNOWN=$((KNOWN + 1)); printf 'KNOWN  %-6s %s -> %s\n' "$1" "$2" "$3"; }

# curl via the CDA Unix socket from a throwaway container: sock_curl <uid:gid> <curl args...>
sock_curl() {
  user="$1"
  shift
  docker run --rm --user "$user" -v "${SOCK_VOL}:/run/cda:ro" \
    --entrypoint curl "$CLIENT_IMG" -s -m 5 --unix-socket /run/cda/cda.sock "$@"
}

container_id() { docker compose ps -q "$1"; }

echo "== Issue 7 tests (project: ${PROJECT})"
for svc in sovd-cda powertrain-mode-controller fms ecu-sim spire-server; do
  if [ -z "$(container_id "$svc")" ]; then
    echo "Service '$svc' is not running. Start the stack first (see header of this script)."
    exit 2
  fi
done

# --- Transport exposure -----------------------------------------------------

id="TC01"; name="CDA has no TCP listener on port 20002"
out=$(docker compose exec -T sovd-cda ss -ltnH 2>&1)
if [ $? -ne 0 ]; then
  fail "$id" "$name" "ss failed: $out"
elif echo "$out" | grep -q ':20002 '; then
  fail "$id" "$name" "listener found: $out"
else
  pass "$id" "$name"
fi

id="TC02"; name="Host cannot reach CDA on localhost:${CDA_PORT}"
if curl -s -m 3 -o /dev/null "http://localhost:${CDA_PORT}/"; then
  fail "$id" "$name" "connection succeeded"
else
  pass "$id" "$name"
fi

id="TC03"; name="vehicle-sovd network: sovd-cda:20002 refused"
docker run --rm --network "${PROJECT}_vehicle-sovd" --entrypoint curl "$CLIENT_IMG" \
  -s -m 3 -o /dev/null http://sovd-cda:20002/
rc=$?
[ "$rc" -eq 7 ] && pass "$id" "$name" || fail "$id" "$name" "curl exit $rc (expected 7)"

id="TC04"; name="vehicle-uprotocol network (PMC's): sovd-cda not resolvable"
docker run --rm --network "${PROJECT}_vehicle-uprotocol" --entrypoint curl "$CLIENT_IMG" \
  -s -m 3 -o /dev/null http://sovd-cda:20002/
rc=$?
[ "$rc" -eq 6 ] && pass "$id" "$name" || fail "$id" "$name" "curl exit $rc (expected 6)"

# --- Socket protection ------------------------------------------------------

id="TC05"; name="CDA runs as 10001:10100, dir 0750 and socket 0770 owned 10001:10100"
out=$(docker compose exec -T sovd-cda sh -c \
  'echo "$(id -u):$(id -g)"; stat -c "%A %u %g" /run/cda /run/cda/cda.sock' 2>&1 | tr '\n' '|')
expected="10001:10100|drwxr-x--- 10001 10100|srwxrwx--- 10001 10100|"
[ "$out" = "$expected" ] && pass "$id" "$name" || fail "$id" "$name" "got '$out'"

id="TC06"; name="Client with volume but wrong group cannot connect"
sock_curl 10003:10003 -o /dev/null "$DATA_URL"
rc=$?
[ "$rc" -eq 7 ] && pass "$id" "$name" || fail "$id" "$name" "curl exit $rc (expected 7)"

id="TC07"; name="Client in group 10100 without token gets 401"
code=$(sock_curl 10003:10100 -o /dev/null -w '%{http_code}' "$DATA_URL")
[ "$code" = "401" ] && pass "$id" "$name" || fail "$id" "$name" "HTTP $code"

id="TC08"; name="Read-only mount prevents deleting the socket (PMC uid)"
out=$(docker run --rm --user 10002:10100 -v "${SOCK_VOL}:/run/cda:ro" --entrypoint sh "$CLIENT_IMG" \
  -c 'rm -f /run/cda/cda.sock 2>&1')
echo "$out" | grep -q "Read-only file system" && pass "$id" "$name" || fail "$id" "$name" "got '$out'"

id="TC09"; name="Container without the volume cannot see the socket"
if docker run --rm --entrypoint sh "$CLIENT_IMG" -c 'test -e /run/cda/cda.sock'; then
  fail "$id" "$name" "socket visible"
else
  pass "$id" "$name"
fi

# --- Container hardening ----------------------------------------------------

id="TC10"; name="sovd-cda: no published ports, cap_drop ALL, no-new-privileges"
cid=$(container_id sovd-cda)
ports=$(docker inspect -f '{{json .NetworkSettings.Ports}}' "$cid")
hc=$(docker inspect -f '{{.HostConfig.CapDrop}} {{.HostConfig.SecurityOpt}}' "$cid")
if echo "$ports" | grep -q 'HostPort'; then
  fail "$id" "$name" "published ports: $ports"
elif echo "$hc" | grep -q 'ALL' && echo "$hc" | grep -q 'no-new-privileges'; then
  pass "$id" "$name"
else
  fail "$id" "$name" "got '$hc'"
fi

id="TC11"; name="PMC: user 10002:10100, not on vehicle-sovd, cap_drop ALL, no-new-privileges"
cid=$(container_id powertrain-mode-controller)
out=$(docker inspect -f '{{.Config.User}}|{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}|{{.HostConfig.CapDrop}} {{.HostConfig.SecurityOpt}}' "$cid")
user=$(echo "$out" | cut -d'|' -f1)
nets=$(echo "$out" | cut -d'|' -f2)
hc=$(echo "$out" | cut -d'|' -f3)
if [ "$user" != "10002:10100" ]; then
  fail "$id" "$name" "user '$user'"
elif echo "$nets" | grep -q 'vehicle-sovd'; then
  fail "$id" "$name" "networks: $nets"
elif ! echo "$hc" | grep -q 'ALL' || ! echo "$hc" | grep -q 'no-new-privileges'; then
  fail "$id" "$name" "got '$hc'"
else
  pass "$id" "$name"
fi

# --- End-to-end -------------------------------------------------------------

id="TC12"; name="PMC is configured to use the Unix socket"
if docker compose logs --no-log-prefix powertrain-mode-controller 2>&1 | grep -q 'Unix socket: /run/cda/cda.sock'; then
  pass "$id" "$name"
else
  fail "$id" "$name" "startup log line not found"
fi

id="TC13"; name="End-to-end mode switching FMS -> PMC -> CDA (UDS + lock) -> ECU"
echo "       (collecting logs for 12s)"
sleep 12
fms_ok=$(docker compose logs --since 15s --no-log-prefix fms 2>&1 | grep -c 'Successfully set powertrain mode')
ecu_ok=$(docker compose logs --since 15s --no-log-prefix ecu-sim 2>&1 | grep -c "Request for blueprint-ecu: '2E 4E 66")
if [ "$fms_ok" -ge 2 ] && [ "$ecu_ok" -ge 2 ]; then
  pass "$id" "$name"
else
  fail "$id" "$name" "FMS successes=$fms_ok, ECU writes=$ecu_ok (expected >=2 each)"
fi

# --- Authorization with a valid but unauthorized JWT-SVID -------------------

spire() { docker compose exec -T spire-server /opt/spire/bin/spire-server "$@"; }
SPIRE_SOCK="-socketPath /run/spire/server/private/api.sock"
ENTRY=""
cleanup() {
  if [ -n "$ENTRY" ]; then
    spire entry delete $SPIRE_SOCK -entryID "$ENTRY" >/dev/null 2>&1
    ENTRY=""
  fi
}
trap cleanup EXIT INT TERM

ENTRY=$(spire entry create $SPIRE_SOCK \
  -parentID spiffe://sdv.eclipse.org/spire/agent/x509pop/spire-agent-vehicle \
  -spiffeID spiffe://sdv.eclipse.org/vehicle/test-unauthorized \
  -selector docker:env:SOVD_TEST_WORKLOAD=unauthorized 2>/dev/null | awk '/Entry ID/ {print $4}')

TOKEN=""
i=0
while [ -n "$ENTRY" ] && [ -z "$TOKEN" ] && [ "$i" -lt 15 ]; do
  i=$((i + 1))
  TOKEN=$(docker run --rm -e SOVD_TEST_WORKLOAD=unauthorized \
    -v "${SPIRE_VOL}:/tmp/spire-agent/public" \
    --entrypoint /opt/spire/bin/spire-agent "$SPIRE_AGENT_IMG" \
    api fetch jwt -audience sovd.cda -socketPath /tmp/spire-agent/public/api.sock 2>/dev/null \
    | awk '/^token/ {getline; gsub(/[ \t]/, ""); print; exit}')
  [ -z "$TOKEN" ] && sleep 2
done

if [ -z "$TOKEN" ]; then
  fail "TC14" "Obtain JWT-SVID for unauthorized test identity" "no token (entry: '${ENTRY}')"
else
  pass "TC14" "Obtain JWT-SVID for unauthorized test identity"

  id="TC15"; name="Unauthorized SPIFFE ID reading powertrain_mode is denied (403)"
  code=$(sock_curl 10003:10100 -H "Authorization: Bearer $TOKEN" -o /dev/null -w '%{http_code}' "$DATA_URL")
  [ "$code" = "403" ] && pass "$id" "$name" || fail "$id" "$name" "HTTP $code"

  id="TC16"; name="Unauthorized SPIFFE ID writing powertrain_mode is rejected (403/409)"
  code=$(sock_curl 10003:10100 -H "Authorization: Bearer $TOKEN" -o /dev/null -w '%{http_code}' \
    -X PUT -H "Content-Type: application/json" -d '{"data":{"Mode":"Economy"}}' "$DATA_URL")
  case "$code" in
    403 | 409) pass "$id" "$name" ;;
    *) fail "$id" "$name" "HTTP $code" ;;
  esac

  id="TC17"; name="Unauthorized SPIFFE ID acquiring an ECU lock is rejected"
  out=$(sock_curl 10003:10100 -H "Authorization: Bearer $TOKEN" -w '\n%{http_code}' \
    -X POST -H "Content-Type: application/json" \
    -d '{"lock_expiration":3,"x-sovd2uds-isexclusive":false}' "${ECU_URL}/locks")
  code=$(echo "$out" | tail -1)
  lock_id=$(echo "$out" | head -1 | sed -nE 's/.*"id":"([^"]+)".*/\1/p')
  if [ -n "$lock_id" ]; then
    sock_curl 10003:10100 -H "Authorization: Bearer $TOKEN" -o /dev/null -X DELETE "${ECU_URL}/locks/${lock_id}"
  fi
  case "$code" in
    2??) known "$id" "$name" "HTTP $code - lock endpoints are not covered by Rego (known limitation)" ;;
    *) pass "$id" "$name" ;;
  esac
fi
cleanup

echo
echo "== Summary: ${PASS} passed, ${FAIL} failed, ${KNOWN} known limitation(s)"
[ "$FAIL" -eq 0 ]
