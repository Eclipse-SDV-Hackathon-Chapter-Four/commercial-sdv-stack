#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 two-board deployment
#
# Copies the board stacks from the Mac to both i.MX95 boards and starts them.
# Board B is only reachable through board A (ssh -J). Images must already be loaded on the boards.

set -e

BOARD_A=${BOARD_A:-root@192.168.2.10}
BOARD_B=${BOARD_B:-root@10.2.0.11}
DEST=${DEST:-/opt/sdv-imx95}

here=$(cd "$(dirname "$0")" && pwd)
config="$here/../../config"

for f in "$here/certs/spire-agent-imx95-cert.pem" "$here/certs/spire-agent-imx95-key.pem"; do
  [ -f "$f" ] || { echo "missing $f, run create_agent_cert.sh first" >&2; exit 1; }
done

# the boards have no RTC/NTP; SPIRE rejects certificates if their clocks are off
ssh "$BOARD_A" "date -u -s @$(date -u +%s) >/dev/null"
ssh -J "$BOARD_A" "$BOARD_B" "date -u -s @$(date -u +%s) >/dev/null"

echo "board B: ecu-sim"
ssh -J "$BOARD_A" "$BOARD_B" "mkdir -p $DEST"
scp -q -J "$BOARD_A" "$here/board-b/docker-compose.yaml" "$BOARD_B:$DEST/"
ssh -J "$BOARD_A" "$BOARD_B" "cd $DEST && docker compose up -d --wait"

echo "board A: SPIRE agent, CDA, PMC"
ssh "$BOARD_A" "mkdir -p $DEST/config/cda $DEST/config/spire/agent $DEST/config/spire/certs"
scp -q "$here/board-a/docker-compose.yaml" "$BOARD_A:$DEST/"
scp -q -r "$config/cda/config" "$config/cda/odx" "$BOARD_A:$DEST/config/cda/"
scp -q -r "$config/powertrain-mode-controller" "$BOARD_A:$DEST/config/"
scp -q "$config/spire/agent/agent.conf" "$BOARD_A:$DEST/config/spire/agent/"
scp -q "$config/spire/certs/trusted-certs.pem" "$here/certs/spire-agent-imx95-cert.pem" \
  "$here/certs/spire-agent-imx95-key.pem" "$BOARD_A:$DEST/config/spire/certs/"
scp -q "$here/board-a/dozzle-relay-b.service" "$BOARD_A:/etc/systemd/system/"
ssh "$BOARD_A" "chmod 600 $DEST/config/spire/certs/*-key.pem \
  && systemctl daemon-reload && systemctl enable --now dozzle-relay-b.service \
  && cd $DEST && docker compose up -d"
