#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Runs on board A: creates an MQTT user with a random password once and stores the password
# in ../.env (read by docker compose). Usage: add-user.sh <user> <.env variable>

set -e

user=$1
var=$2
cd "$(dirname "$0")/.."

touch .env
chmod 600 .env
if [ -f mosquitto/passwd ] && grep -q "^$user:" mosquitto/passwd && grep -q "^$var=" .env; then
  exit 0
fi

pw=$(openssl rand -hex 16)
touch mosquitto/passwd
docker run --rm --network none -v "$PWD/mosquitto:/m" eclipse-mosquitto:2 \
  mosquitto_passwd -b /m/passwd "$user" "$pw"
chown 1883:1883 mosquitto/passwd
chmod 600 mosquitto/passwd
sed -i "/^$var=/d" .env
echo "$var=$pw" >> .env
echo "created MQTT user $user"
