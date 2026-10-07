#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Manages the AutoSD VM (UTM, QEMU user networking) that runs the backend containers.
#   vm.sh create <autosd.qcow2>  creates the VM, reachable only via ssh -p 2222 root@127.0.0.1
#   vm.sh publish                forwards the backend ports (SPIRE, MQTT, NTP) on MAC_IP to the VM;
#                                the Docker Desktop backend must be stopped first
#   vm.sh unpublish              removes these forwards again
# UTM 4.5 cannot set port forwards via AppleScript, so they are written to the VM's config.plist,
# which UTM only reads at app start; no other VM may be running.

set -e

VM_NAME=${VM_NAME:-autosd-backend}
MAC_IP=${MAC_IP:-192.168.2.101}
DATABROKER_PORT=${DATABROKER_PORT:-55556}
SSH_PORT=${SSH_PORT:-2222}
PLIST="$HOME/Library/Containers/com.utmapp.UTM/Data/Documents/$VM_NAME.utm/config.plist"

utmctl() {
  /Applications/UTM.app/Contents/MacOS/utmctl "$@"
}

forward() {
  printf '{"Protocol":"%s","HostAddress":"%s","HostPort":%s,"GuestAddress":"","GuestPort":%s}' \
    "$1" "$2" "$3" "$4"
}

forwards() {
  list=$(forward TCP 127.0.0.1 "$SSH_PORT" 22)
  if [ "$1" = publish ]; then
    # macOS lets non-root processes bind ports below 1024 only on all addresses
    for f in "TCP $MAC_IP 8081 8081" "TCP $MAC_IP 1883 1883" "TCP 127.0.0.1 1883 1883" \
      "UDP 0.0.0.0 123 123" "TCP 127.0.0.1 8080 8080" "TCP 127.0.0.1 $DATABROKER_PORT 55555"; do
      # shellcheck disable=SC2086
      set -- $f
      list="$list,$(forward "$@")"
    done
  fi
  printf '[%s]' "$list"
}

set_forwards() {
  if utmctl list | grep -v -w "$VM_NAME" | grep -q -w -E 'started|starting|paused'; then
    echo "stop the other UTM VMs first, UTM has to be restarted" >&2
    exit 1
  fi
  if utmctl status "$VM_NAME" | grep -q -v stopped; then
    utmctl stop "$VM_NAME"
    until utmctl status "$VM_NAME" | grep -q stopped; do sleep 1; done
  fi
  osascript -e 'tell application "UTM" to quit'
  while pgrep -x UTM >/dev/null; do sleep 1; done
  plutil -replace Network.0.PortForward -json "$(forwards "$1")" "$PLIST"
  open -g -a UTM
  until utmctl list >/dev/null 2>&1; do sleep 1; done
  utmctl start "$VM_NAME"
}

case "$1" in
  create)
    image=${2:?usage: vm.sh create <autosd.qcow2>}
    osascript <<EOF
set disk to POSIX file "$image"
tell application "UTM"
  make new virtual machine with properties {backend:qemu, configuration:{name:"$VM_NAME", architecture:"aarch64", memory:4096, cpu cores:4, drives:{{removable:false, interface:VirtIO, source:disk}}, network interfaces:{{mode:emulated}}}}
end tell
EOF
    set_forwards
    ;;
  publish | unpublish)
    set_forwards "$1"
    ;;
  *)
    echo "usage: vm.sh create <autosd.qcow2> | publish | unpublish" >&2
    exit 2
    ;;
esac
