#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 two-board deployment
#
# Starts only the backend containers on the Mac (CDA, PMC and ecu-sim run on the boards)
# and registers the workloads with SPIRE. Set DATABROKER_PORT if 55555 is taken on the Mac.

set -e

cd "$(dirname "$0")/../.."

compose() {
  docker compose -f docker-compose.yaml -f deploy/imx95/docker-compose.mac.yaml \
    --profile infra --profile fw-update --profile powertrain "$@"
}

compose --profile board rm -f -s sovd-cda powertrain-mode-controller ecu-sim
compose up -d

i=0
until compose exec -T spire-server /opt/spire/bin/spire-server healthcheck \
  -socketPath /run/spire/server/private/api.sock >/dev/null 2>&1; do
  i=$((i + 1))
  [ "$i" -lt 30 ] || { echo "spire-server not healthy" >&2; exit 1; }
  sleep 1
done

# entries survive container restarts but not re-creation; creating existing ones fails harmlessly
./scripts/register_workloads.sh >/dev/null 2>&1 || true
./deploy/imx95/register_workloads.sh >/dev/null 2>&1 || true
compose exec -T spire-server /opt/spire/bin/spire-server entry show \
  -socketPath /run/spire/server/private/api.sock | grep "SPIFFE ID" | sort | uniq -c

compose ps --format "{{.Service}}: {{.Status}}"
