#!/bin/sh
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - i.MX95 two-board deployment
#
# Creates board A's SPIRE agent key and certificate, signed by the existing CA in config/spire/certs.
# Board A needs its own agent identity: the Mac keeps running spire-agent-vehicle for vehicle-properties.

set -e

name=spire-agent-imx95
here=$(cd "$(dirname "$0")" && pwd)
ca_dir="$here/../../config/spire"
out="$here/certs"

if [ -f "$out/$name-cert.pem" ]; then
  echo "$out/$name-cert.pem already exists"
  exit 0
fi

mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf '[ req_ext ]\nsubjectKeyIdentifier = hash\nkeyUsage = keyAgreement,keyEncipherment,digitalSignature\nextendedKeyUsage = serverAuth, clientAuth\nsubjectAltName = DNS.1:%s\n' "$name" > "$tmp/ext.cnf"

openssl ecparam -name secp384r1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt -out "$out/$name-key.pem"
chmod 600 "$out/$name-key.pem"
openssl req -config "$ca_dir/ca_opts" -new -key "$out/$name-key.pem" \
  -subj "/C=BE/L=Brussels/O=Eclipse SDV/OU=SPIRE/CN=$name" |
  openssl x509 -req -extfile "$tmp/ext.cnf" -extensions req_ext -days 365 \
    -CA "$ca_dir/certs/ca-cert.pem" -CAkey "$ca_dir/certs/ca-key.pem" \
    -CAserial "$tmp/ca.srl" -CAcreateserial -out "$tmp/cert.pem"
cat "$tmp/cert.pem" "$ca_dir/certs/ca-cert.pem" > "$out/$name-cert.pem"

echo "created $out/$name-cert.pem and $out/$name-key.pem"
