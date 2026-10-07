#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 two-board deployment
#
# Copies the board stacks from the Mac to both i.MX95 boards and starts them. Containers restart
# on power-up after the boards' first NTP sync with the Mac (backend.sh runs the NTP server).
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

# initial clock set; after this, timesyncd keeps the boards in sync with the Mac's NTP server
ssh "$BOARD_A" "date -u -s @$(date -u +%s) >/dev/null"
ssh -J "$BOARD_A" "$BOARD_B" "date -u -s @$(date -u +%s) >/dev/null"

# $1 = ssh/scp jump options, $2 = board, $3 = board directory
install_time_sync() {
  ssh $1 "$2" "mkdir -p /etc/systemd/timesyncd.conf.d"
  scp -q $1 "$here/$3/timesyncd.conf" "$2:/etc/systemd/timesyncd.conf.d/sdv-imx95.conf"
  ssh $1 "$2" "systemctl restart systemd-timesyncd"
}

echo "board B: ecu-sim"
install_time_sync "-J $BOARD_A" "$BOARD_B" board-b
# board B has no SPIRE workloads, so its containers need not wait for time sync at boot
ssh -J "$BOARD_A" "$BOARD_B" "rm -f /etc/systemd/system/docker.service.d/10-wait-time-sync.conf \
  /etc/systemd/system/systemd-time-wait-sync.service.d/10-timeout.conf && systemctl daemon-reload"
ssh -J "$BOARD_A" "$BOARD_B" "mkdir -p $DEST"
scp -q -J "$BOARD_A" "$here/board-b/docker-compose.yaml" "$BOARD_B:$DEST/"
ssh -J "$BOARD_A" "$BOARD_B" "cd $DEST && docker compose up -d --wait"

echo "board A: SPIRE agent, CDA, PMC"
install_time_sync "" "$BOARD_A" board-a
ssh "$BOARD_A" "mkdir -p /etc/systemd/system/docker.service.d /etc/systemd/system/systemd-time-wait-sync.service.d"
scp -q "$here/boot/docker-wait-time-sync.conf" "$BOARD_A:/etc/systemd/system/docker.service.d/10-wait-time-sync.conf"
scp -q "$here/boot/time-wait-sync-timeout.conf" \
  "$BOARD_A:/etc/systemd/system/systemd-time-wait-sync.service.d/10-timeout.conf"
ssh "$BOARD_A" "mkdir -p $DEST/config/cda $DEST/config/spire/agent $DEST/config/spire/certs"
scp -q "$here/board-a/docker-compose.yaml" "$BOARD_A:$DEST/"
scp -q -r "$config/cda/config" "$config/cda/odx" "$BOARD_A:$DEST/config/cda/"
scp -q -r "$config/powertrain-mode-controller" "$BOARD_A:$DEST/config/"
scp -q "$config/spire/agent/agent.conf" "$BOARD_A:$DEST/config/spire/agent/"
scp -q "$config/spire/certs/trusted-certs.pem" "$here/certs/spire-agent-imx95-cert.pem" \
  "$here/certs/spire-agent-imx95-key.pem" "$BOARD_A:$DEST/config/spire/certs/"
scp -q "$here/board-a/dozzle-relay-b.service" "$BOARD_A:/etc/systemd/system/"
scp -q -r "$here/board-a/mosquitto" "$BOARD_A:$DEST/"
# broker passwords are generated once on the board and kept in $DEST/.env (read by docker compose)
ssh "$BOARD_A" "set -e; sh $DEST/mosquitto/add-user.sh pmc STATUS_MQTT_PASSWORD \
  && sh $DEST/mosquitto/add-user.sh anomaly-detector ANOMALY_MQTT_PASSWORD"
# SIGHUP makes a running broker reload the password file and ACL
ssh "$BOARD_A" "chmod 600 $DEST/config/spire/certs/*-key.pem \
  && systemctl daemon-reload && systemctl enable --now dozzle-relay-b.service \
  && cd $DEST && docker compose up -d && docker compose kill -s HUP mosquitto"
