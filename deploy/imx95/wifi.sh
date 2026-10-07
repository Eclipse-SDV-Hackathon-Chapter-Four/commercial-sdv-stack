#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 Wi-Fi MQTT
#
# Joins board A to a Wi-Fi access point. Only DHCP, ICMP, mDNS and MQTT are reachable over Wi-Fi.
# Usage: WIFI_SSID=SDVTeam1 ./deploy/imx95/wifi.sh   (asks for the passphrase unless WIFI_PASSPHRASE is set)

set -e

BOARD_A=${BOARD_A:-root@192.168.2.10}
BOARD_A_NAME=${BOARD_A_NAME:-imx95-a}
SSID=${WIFI_SSID:?set WIFI_SSID}
files=$(cd "$(dirname "$0")" && pwd)/board-a/wifi

if [ -z "$WIFI_PASSPHRASE" ]; then
  printf 'Passphrase for %s: ' "$SSID"
  stty -echo
  read -r WIFI_PASSPHRASE
  stty echo
  echo
fi

ssh "$BOARD_A" "mkdir -p /etc/systemd/system/wpa_supplicant@mlan0.service.d /etc/systemd/network/20-mlan0.network.d"
scp -q "$files/sdv-wifi-firewall.nft" "$BOARD_A:/etc/sdv-wifi-firewall.nft"
scp -q "$files/sdv-wifi-firewall.service" "$BOARD_A:/etc/systemd/system/"
scp -q "$files/wpa_supplicant-firewall.conf" "$BOARD_A:/etc/systemd/system/wpa_supplicant@mlan0.service.d/10-firewall.conf"
scp -q "$files/mlan0-offline.conf" "$BOARD_A:/etc/systemd/network/20-mlan0.network.d/10-offline.conf"

# the firewall must be active before the board joins any Wi-Fi
ssh "$BOARD_A" "systemctl daemon-reload && systemctl enable --now sdv-wifi-firewall.service \
  && nft list table inet sdv_wifi >/dev/null"

# WPA2 PSK = PBKDF2-SHA1(passphrase, SSID, 4096, 32); only the derived key leaves the Mac
psk=$(printf '%s' "$WIFI_PASSPHRASE" | SSID="$SSID" python3 -c \
  'import hashlib, os, sys; print(hashlib.pbkdf2_hmac("sha1", sys.stdin.read().encode(), os.environ["SSID"].encode(), 4096, 32).hex())')
ssh "$BOARD_A" "conf=/etc/wpa_supplicant/wpa_supplicant-mlan0.conf
  grep -q 'ssid=\"$SSID\"' \$conf || printf 'network={\n\tssid=\"%s\"\n\tpsk=%s\n}\n' '$SSID' '$psk' >> \$conf
  chmod 600 \$conf"

ssh "$BOARD_A" "hostnamectl set-hostname $BOARD_A_NAME && systemctl restart avahi-daemon \
  && networkctl reload && systemctl enable wpa_supplicant@mlan0.service \
  && systemctl restart wpa_supplicant@mlan0.service"

i=0
until ip=$(ssh "$BOARD_A" "ip -4 -br addr show mlan0" | awk '{print $3}' | cut -d/ -f1) && [ -n "$ip" ]; do
  i=$((i + 1))
  [ "$i" -lt 20 ] || { echo "no Wi-Fi address yet; check: ssh $BOARD_A journalctl -u wpa_supplicant@mlan0" >&2; exit 1; }
  sleep 2
done
echo "board A on $SSID: $ip ($BOARD_A_NAME.local)"
