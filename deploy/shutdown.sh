#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Shuts the demo down cleanly from the Mac: board B, then board A, then the AutoSD VM.
# Unplug the boards' power once their LEDs are off; everything restarts with deploy/startup.sh.

BOARD_A=${BOARD_A:-root@192.168.2.10}
BOARD_B=${BOARD_B:-root@10.2.0.11}
VM_NAME=${VM_NAME:-autosd-backend}
SSH="ssh -o BatchMode=yes -o ConnectTimeout=5"

utmctl() {
  /Applications/UTM.app/Contents/MacOS/utmctl "$@"
}

echo "== board B"
if $SSH -J "$BOARD_A" "$BOARD_B" poweroff 2>/dev/null; then
  echo "powering off"
else
  echo "not reachable, skipping"
fi

echo "== board A"
# give board B's shutdown a moment, it is only reachable through board A
sleep 10
if $SSH "$BOARD_A" poweroff 2>/dev/null; then
  echo "powering off"
else
  echo "not reachable, skipping"
fi

echo "== backend (AutoSD VM)"
if utmctl status "$VM_NAME" 2>/dev/null | grep -q started; then
  utmctl stop "$VM_NAME"
  i=0
  while ! utmctl status "$VM_NAME" | grep -q stopped; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || { echo "VM still running; stop it in UTM" >&2; break; }
    sleep 2
  done
  echo "stopped"
else
  echo "not running"
fi

echo
echo "Demo is down. Unplug the boards once their LEDs are off."
