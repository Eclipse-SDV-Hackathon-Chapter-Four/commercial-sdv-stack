#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Runs inside the AutoSD VM (copied there by backend.sh, started at boot by sdv-backend.service):
# starts the backend without Symphony and registers the workloads with SPIRE.

set -e

cd "$(dirname "$0")/../.."

compose() {
  docker compose -f docker-compose.yaml -f deploy/imx95/docker-compose.mac.yaml \
    -f deploy/autosd/docker-compose.autosd.yaml --profile infra --profile powertrain "$@"
}

compose up -d --no-build --pull never
compose --profile fw-update up -d --no-build --pull never --no-deps ecu-updater

i=0
until compose exec -T spire-server /opt/spire/bin/spire-server healthcheck \
  -socketPath /run/spire/server/private/api.sock >/dev/null 2>&1; do
  i=$((i + 1))
  [ "$i" -lt 30 ] || { echo "spire-server not healthy" >&2; exit 1; }
  sleep 1
done

./scripts/register_workloads.sh >/dev/null 2>&1 || true
./deploy/imx95/register_workloads.sh >/dev/null 2>&1 || true
compose exec -T spire-server /opt/spire/bin/spire-server entry show \
  -socketPath /run/spire/server/private/api.sock | grep "SPIFFE ID" | sort | uniq -c

compose --profile fw-update ps --format "{{.Service}}: {{.Status}}"
