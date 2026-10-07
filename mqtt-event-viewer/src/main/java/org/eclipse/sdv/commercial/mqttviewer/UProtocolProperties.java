/*
 * SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Apache License Version 2.0 which is available at
 * https://www.apache.org/licenses/LICENSE-2.0
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Portions of this file were generated with AI assistance (GitHub Copilot).
 */
package org.eclipse.sdv.commercial.mqttviewer;

import jakarta.validation.constraints.NotBlank;
import java.time.Duration;
import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.boot.context.properties.bind.DefaultValue;
import org.springframework.validation.annotation.Validated;

/**
 * Configuration of the viewer's uProtocol endpoint.
 *
 * @param enabled    {@code true} to connect to the broker as a uProtocol entity (using the MQTT 5 transport),
 *                   {@code false} to subscribe to the configured raw MQTT topic filters instead
 * @param localUri   the viewer's own uProtocol URI, e.g. {@code up://backend/103AB/1/0}
 * @param rpcEnabled {@code true} to allow invoking uProtocol service operations via the HTTP API
 * @param rpcTimeout default time to wait for the response to an RPC request
 */
@Validated
@ConfigurationProperties(prefix = "uprotocol")
public record UProtocolProperties(
        @DefaultValue("true") boolean enabled,
        @DefaultValue("up://backend/103AB/1/0") @NotBlank String localUri,
        @DefaultValue("true") boolean rpcEnabled,
        @DefaultValue("5s") Duration rpcTimeout) {
}
