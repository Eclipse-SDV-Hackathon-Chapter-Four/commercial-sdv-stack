#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Turns board A's Wi-Fi into a 2.4 GHz access point for the MXChip AZ3166 (which has no 5 GHz):
# SSID $AP_SSID, board A at 192.168.60.1 with DHCP, DNS, NTP and MQTT. The NTP pool names resolve
# to board A, as the MXChip needs the time and there is no internet behind the access point.
# Replaces the Wi-Fi client; back to it with:
#   ssh root@192.168.2.10 'systemctl disable --now sdv-wifi-ap && systemctl enable --now wpa_supplicant@mlan0'

set -e

BOARD_A=${BOARD_A:-root@192.168.2.10}
SSID=${AP_SSID:-SDV-imx95-A}
PASSPHRASE=${AP_PASSPHRASE:?set AP_PASSPHRASE (8 to 63 characters)}
files=$(cd "$(dirname "$0")" && pwd)/board-a/wifi
# time servers commonly built into firmware; all resolve to board A
NTP_NAMES="pool.ntp.org 0.pool.ntp.org 1.pool.ntp.org 2.pool.ntp.org 3.pool.ntp.org \
time.windows.com time.google.com time1.google.com time.nist.gov time.cloudflare.com \
time.apple.com ntp.ubuntu.com europe.pool.ntp.org de.pool.ntp.org"

ssh "$BOARD_A" "mkdir -p /etc/systemd/resolved.conf.d"
scp -q "$files/sdv-wifi-firewall.nft" "$BOARD_A:/etc/sdv-wifi-firewall.nft"
scp -q "$files/sdv-wifi-ap.service" "$BOARD_A:/etc/systemd/system/"
scp -q "$files/30-uap0.network" "$BOARD_A:/etc/systemd/network/"
scp -q "$files/resolved-ap.conf" "$BOARD_A:/etc/systemd/resolved.conf.d/sdv-wifi-ap.conf"
printf 'interface=uap0\ndriver=nl80211\nssid=%s\nhw_mode=g\nchannel=6\nieee80211n=1\nwpa=2\nwpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\nwpa_passphrase=%s\n' \
  "$SSID" "$PASSPHRASE" | ssh "$BOARD_A" "umask 077 && cat > /etc/sdv-wifi-ap.conf"
ssh "$BOARD_A" "sed -i '/# sdv-wifi-ap\$/d' /etc/hosts && echo '192.168.60.1 $NTP_NAMES # sdv-wifi-ap' >> /etc/hosts"

ssh "$BOARD_A" "systemctl daemon-reload && systemctl restart sdv-wifi-firewall systemd-resolved \
  && systemctl disable --now wpa_supplicant@mlan0 && networkctl reload \
  && systemctl enable --now sdv-wifi-ap"

i=0
until ssh "$BOARD_A" "systemctl is-active -q sdv-wifi-ap && ip -4 -br addr show uap0 | grep -q 192.168.60.1"; do
  i=$((i + 1))
  [ "$i" -lt 15 ] || { echo "access point not up; check: ssh $BOARD_A journalctl -u sdv-wifi-ap" >&2; exit 1; }
  sleep 2
done
echo "access point $SSID up; MQTT broker for its clients: 192.168.60.1:1883"
