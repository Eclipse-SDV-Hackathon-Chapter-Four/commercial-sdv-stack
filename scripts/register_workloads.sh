#!/bin/bash

#*******************************************************************************
# Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0 which is available at
# http://www.eclipse.org/legal/epl-2.0
#
# SPDX-License-Identifier: EPL-2.0
#*******************************************************************************
APPROVED_FILE="$(dirname "$0")/../config/spire/approved-workloads.list"

#  Helper to access spire server functions
spire_server() {
  # -T + </dev/null: keep docker exec from consuming the loop's stdin
  docker compose exec -T spire-server \
    /opt/spire/bin/spire-server "$@" \
      -socketPath /run/spire/server/private/api.sock </dev/null
}

#  Remove existing registrations
#  For the purpose of demo/blueprint this is fine
spire_server entry show | awk '/^Entry ID/ {print $4}' | while read -r id; do
  echo "deleting entry $id"
  spire_server entry delete -entryID "$id" >/dev/null
done

#  Registers the workloads with the SPIRE server
while read -r -u 3 spiffe_id digest _ || [[ -n "$spiffe_id" ]]; do
  [[ -z "$spiffe_id" || "$spiffe_id" == \#* ]] && continue

  zone=$(echo "$spiffe_id" | cut -d/ -f4)
  case "$zone" in
    backend) agent=spire-agent-backend ;;
    vehicle) agent=spire-agent-vehicle ;;
    *) echo "unknown zone '$zone' in $spiffe_id" >&2; exit 1 ;;
  esac

  spire_server entry create \
    -parentID "spiffe://sdv.eclipse.org/spire/agent/x509pop/${agent}" \
    -spiffeID "$spiffe_id" \
    -selector "docker:image_config_digest:${digest}"
done 3< "$APPROVED_FILE"