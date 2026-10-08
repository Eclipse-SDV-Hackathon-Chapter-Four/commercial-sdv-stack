#!/usr/bin/env bash
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Apache License Version 2.0 which is available at
# https://www.apache.org/licenses/LICENSE-2.0
#
# SPDX-License-Identifier: Apache-2.0
#
# Portions of this file were generated with AI assistance (GitHub Copilot).
#
# Builds Eclipse uProtocol's MQTT 5 transport for Java and installs it into the local Maven repository.
#
# The library has not been released to Maven Central yet. Its main branch depends on a
# SNAPSHOT version of up-java which is no longer available, so the released up-java version
# is used instead.

set -euo pipefail

UP_TRANSPORT_MQTT5_JAVA_COMMIT=${UP_TRANSPORT_MQTT5_JAVA_COMMIT:-"76d876c3b63b9f5c44b4210f8157aeb8c7898910"}
UP_JAVA_VERSION=${UP_JAVA_VERSION:-"3.0.0"}
MVN=${MVN:-"mvn"}

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

echo "Downloading up-transport-mqtt5-java@${UP_TRANSPORT_MQTT5_JAVA_COMMIT}..."
curl -fsSL "https://github.com/eclipse-uprotocol/up-transport-mqtt5-java/archive/${UP_TRANSPORT_MQTT5_JAVA_COMMIT}.tar.gz" \
  | tar -xz -C "$work_dir" --strip-components=1

sed -i.bak "s#<version>3.0.0-SNAPSHOT</version>#<version>${UP_JAVA_VERSION}</version>#" "$work_dir/pom.xml"
if ! grep -q "<version>${UP_JAVA_VERSION}</version>" "$work_dir/pom.xml"; then
  echo "Failed to set up-java version in up-transport-mqtt5-java's pom.xml" >&2
  exit 1
fi

echo "Building and installing up-transport-mqtt5-java (using up-java ${UP_JAVA_VERSION})..."
"$MVN" -B -q -f "$work_dir/pom.xml" \
  -DskipTests -Dmaven.javadoc.skip=true -Dmaven.source.skip=true -Dpmd.skip=true -Dcpd.skip=true -Dgpg.skip=true \
  install
echo "Successfully installed org.eclipse.uprotocol:up-transport-mqtt5-java:0.1.0-SNAPSHOT"
