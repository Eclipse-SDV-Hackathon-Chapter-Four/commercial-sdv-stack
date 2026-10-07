#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 two-board deployment
#
# Registers the board A workloads with the SPIRE server on the Mac.
# Run after scripts/register_workloads.sh, again whenever spire-server is recreated.

set -e

cd "$(dirname "$0")/../.."

docker compose exec -T spire-server \
  /opt/spire/bin/spire-server entry create \
    -socketPath /run/spire/server/private/api.sock \
    -parentID spiffe://sdv.eclipse.org/spire/agent/x509pop/spire-agent-imx95 \
    -spiffeID spiffe://sdv.eclipse.org/vehicle/powertrain-mode-controller \
    -selector docker:image_id:ghcr.io/eclipse-sdv-blueprints/commercial-sdv-stack/powertrain-mode-controller:latest \
    -selector docker:env:UP_LOCAL_ADDRESS=up://vehicle/10301/1/0
