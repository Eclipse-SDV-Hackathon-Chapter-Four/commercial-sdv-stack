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

FROM maven:3.9-eclipse-temurin-21 AS builder
WORKDIR /build
COPY install-up-transport-mqtt5.sh .
RUN ./install-up-transport-mqtt5.sh
COPY pom.xml .
RUN mvn -B -q dependency:go-offline
COPY src ./src
RUN mvn -B -q -DskipTests package

FROM eclipse-temurin:21-jre
COPY --from=builder /build/target/mqtt-event-viewer.jar /app/mqtt-event-viewer.jar
EXPOSE 8090
ENTRYPOINT [ "java", "-jar", "/app/mqtt-event-viewer.jar" ]
