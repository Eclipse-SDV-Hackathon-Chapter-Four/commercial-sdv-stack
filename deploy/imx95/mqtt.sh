#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Pauses and resumes all MQTT traffic of the powertrain mode controller and the anomaly
# detector on board A (local broker and backend), without touching the services: their
# packets to port 1883 are dropped by user id. TCP recovers by itself on resume.
#   mqtt.sh pause | resume | status

set -e

BOARD_A=${BOARD_A:-root@192.168.2.10}
# container users: 10002 = powertrain-mode-controller, 65534 = anomaly-detector
UIDS="10002, 65534"

case "$1" in
  pause)
    ssh "$BOARD_A" "nft list table inet sdv_mqtt_pause >/dev/null 2>&1 || { \
      nft add table inet sdv_mqtt_pause \
      && nft add chain inet sdv_mqtt_pause out '{ type filter hook output priority 0; policy accept; }' \
      && nft add rule inet sdv_mqtt_pause out meta skuid { $UIDS } tcp dport 1883 drop; }"
    echo "MQTT paused for the powertrain mode controller and the anomaly detector"
    ;;
  resume)
    ssh "$BOARD_A" "nft delete table inet sdv_mqtt_pause 2>/dev/null || true"
    echo "MQTT resumed"
    ;;
  status)
    if ssh "$BOARD_A" "nft list table inet sdv_mqtt_pause >/dev/null 2>&1"; then
      echo "paused"
    else
      echo "running"
    fi
    ;;
  *)
    echo "usage: mqtt.sh pause | resume | status" >&2
    exit 2
    ;;
esac
