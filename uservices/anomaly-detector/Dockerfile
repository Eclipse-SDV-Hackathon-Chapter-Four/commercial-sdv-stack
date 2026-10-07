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
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
# Build from the uservices directory:
#   docker build -f anomaly-detector/Dockerfile -t ghcr.io/eclipse-sdv-blueprints/commercial-sdv-stack/anomaly-detector:latest .

FROM ghcr.io/rust-cross/rust-musl-cross:x86_64-musl AS chef-amd64
ENV BUILDTARGET="x86_64-unknown-linux-musl"

FROM ghcr.io/rust-cross/rust-musl-cross:aarch64-musl AS chef-arm64
ENV BUILDTARGET="aarch64-unknown-linux-musl"

FROM chef-$TARGETARCH AS chef
ARG TARGETARCH
RUN apt-get update && apt-get install -y ca-certificates \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

ENV CARGO_UNSTABLE_SPARSE_REGISTRY=true
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

USER root
RUN cargo install --locked cargo-chef
WORKDIR /home/rust
RUN echo "Building for $TARGETARCH"

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /home/rust/recipe.json recipe.json
RUN cargo chef cook --release --target $BUILDTARGET --package anomaly-detector --recipe-path recipe.json
COPY . .
RUN cargo build --release --target $BUILDTARGET --package anomaly-detector
RUN mv target/${BUILDTARGET}/release/anomaly-detector target/

FROM scratch

COPY --from=builder /home/rust/target/anomaly-detector /app/anomaly-detector
COPY --from=builder /home/rust/anomaly-detector/training/az3166-baseline.csv /app/training/

ENTRYPOINT [ "/app/anomaly-detector" ]
