#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Starts the whole demo from the Mac and verifies every part of the chain:
# AutoSD VM (backend), board A (CDA, PMC, broker, detector, access point), board B (ecu-sim).
# Power on the boards first; this script waits for them. Safe to run again at any time.

set -e

BOARD_A=${BOARD_A:-root@192.168.2.10}
BOARD_B=${BOARD_B:-root@10.2.0.11}
VM_NAME=${VM_NAME:-autosd-backend}
SSH_PORT=${SSH_PORT:-2222}
AUTOSD_DIR=${AUTOSD_DIR:-$HOME/Work/Hackathon_2026/autosd}
SSH="ssh -o BatchMode=yes -o ConnectTimeout=5"

utmctl() {
  /Applications/UTM.app/Contents/MacOS/utmctl "$@"
}

vm() {
  $SSH -p "$SSH_PORT" -i "$AUTOSD_DIR/id_ed25519" -o LogLevel=error root@127.0.0.1 "$@"
}

# $1 = what, $2 = timeout seconds, rest = command that must succeed
wait_for() {
  what=$1; timeout=$2; shift 2
  start=$(date +%s)
  until "$@" >/dev/null 2>&1; do
    [ $(( $(date +%s) - start )) -lt "$timeout" ] || { echo "FAILED: $what" >&2; exit 1; }
    sleep 3
  done
  echo "ok: $what"
}

echo "== backend (AutoSD VM)"
UTMCTL=/Applications/UTM.app/Contents/MacOS/utmctl
utmctl status "$VM_NAME" | grep -q started || { open -g -a UTM; sleep 3; utmctl start "$VM_NAME"; }
wait_for "VM is running" 60 sh -c "$UTMCTL status $VM_NAME | grep -q started"
wait_for "backend containers are up (needs internet once for time sync)" 300 \
  vm "systemctl is-active -q sdv-backend && [ \$(podman ps -q | wc -l) -ge 10 ]"

echo "== board A"
wait_for "board A answers on the USB LAN" 180 $SSH "$BOARD_A" true
wait_for "board A clock is synced" 120 $SSH "$BOARD_A" \
  "timedatectl show -p NTPSynchronized | grep -q yes"
wait_for "board A containers are up" 180 $SSH "$BOARD_A" \
  "cd /opt/sdv-imx95 && [ \$(docker compose ps -q --status running | wc -l) -ge 7 ]"
wait_for "Wi-Fi access point SDV-imx95-A" 60 $SSH "$BOARD_A" "systemctl is-active -q sdv-wifi-ap"
wait_for "MQTT broker" 60 $SSH "$BOARD_A" \
  "cd /opt/sdv-imx95 && docker compose exec -T mosquitto mosquitto_sub -h 127.0.0.1 -C 1 -W 3 -t vehicle/powertrain/mode"

echo "== board B"
wait_for "board B answers (via board A)" 180 $SSH -J "$BOARD_A" "$BOARD_B" true
wait_for "ecu-sim is healthy" 180 $SSH -J "$BOARD_A" "$BOARD_B" \
  "cd /opt/sdv-imx95 && docker compose ps ecu-sim | grep -q healthy"

echo "== end to end"
wait_for "PMC sets powertrain modes" 120 $SSH "$BOARD_A" \
  "cd /opt/sdv-imx95 && docker compose logs --since 30s powertrain-mode-controller 2>&1 | grep -q 'mode set to'"
wait_for "anomaly detector dashboard" 60 curl -s -f -o /dev/null http://192.168.2.10:8090/api/status

echo
echo "Demo is up."
echo "  Dashboard:  http://192.168.2.10:8090"
echo "  Dozzle:     http://127.0.0.1:8080"
echo "  MXChip:     Wi-Fi SDV-imx95-A, broker 192.168.60.1:1883 (power it on any time)"
echo "  MQTT pause: $(dirname "$0")/imx95/mqtt.sh pause | resume"
