#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Sets up the AutoSD VM (see vm.sh) for the backend, copies the compose files and config into
# it, loads the images from IMAGES (docker save | gzip) if set and starts the backend without
# Symphony. The VM gets everything from the Mac: it cannot verify TLS behind the corporate proxy.

set -e

cd "$(dirname "$0")/../.."

SSH_PORT=${SSH_PORT:-2222}
AUTOSD_DIR=${AUTOSD_DIR:-$HOME/Work/Hackathon_2026/autosd}
SSH_KEY=${SSH_KEY:-$AUTOSD_DIR/id_ed25519}
COMPOSE_BINARY=${COMPOSE_BINARY:-$AUTOSD_DIR/docker-compose-linux-aarch64}
DEST=/opt/sdv-backend

vm() {
  ssh -p "$SSH_PORT" -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
    -o LogLevel=error root@127.0.0.1 "$@"
}

if ! vm test -x /usr/local/bin/docker-compose; then
  vm "cat > /usr/local/bin/docker-compose && chmod 755 /usr/local/bin/docker-compose" \
    <"$COMPOSE_BINARY"
fi
vm sh -s <<'EOF'
set -e
cat > /usr/local/bin/docker <<'WRAPPER'
#!/bin/sh
if [ "$1" = compose ]; then
  shift
  exec /usr/local/bin/docker-compose "$@"
fi
exec podman "$@"
WRAPPER
chmod 755 /usr/local/bin/docker
echo 'L /run/docker.sock - - - - /run/podman/podman.sock' > /etc/tmpfiles.d/docker-sock.conf
systemd-tmpfiles --create /etc/tmpfiles.d/docker-sock.conf
systemctl enable --now podman.socket
systemctl enable chrony-wait.service
EOF

if [ -n "$IMAGES" ]; then
  gunzip -c "$IMAGES" | vm podman load
fi

vm "mkdir -p $DEST"
tar -cf - docker-compose.yaml deploy/imx95/docker-compose.mac.yaml \
  deploy/imx95/register_workloads.sh scripts/register_workloads.sh \
  deploy/autosd/docker-compose.autosd.yaml deploy/autosd/start.sh deploy/autosd/sdv-backend.service \
  config/databroker config/mosquitto config/spire |
  vm "tar -xf - -C $DEST"
vm "chcon -R -t container_file_t $DEST/config \
  && cp $DEST/deploy/autosd/sdv-backend.service /etc/systemd/system/ \
  && systemctl daemon-reload && systemctl enable sdv-backend.service \
  && $DEST/deploy/autosd/start.sh"
