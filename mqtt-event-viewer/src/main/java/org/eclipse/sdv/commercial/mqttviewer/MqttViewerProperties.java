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

import jakarta.validation.constraints.Max;
import jakarta.validation.constraints.Min;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.NotEmpty;
import java.time.Duration;
import java.util.List;
import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.boot.context.properties.bind.DefaultValue;
import org.springframework.validation.annotation.Validated;

/**
 * Configuration of the MQTT connection and the in-memory event buffer.
 *
 * @param brokerUri          URI of the MQTT broker, e.g. {@code tcp://localhost:1883}
 * @param clientId           MQTT client identifier; a random suffix is appended if it ends with {@code -}
 * @param username           optional user name for authenticating with the broker
 * @param password           optional password for authenticating with the broker
 * @param topics             topic filters to subscribe to
 * @param qos                QoS level used for all subscriptions
 * @param bufferSize         number of most recent events kept in memory and replayed to new browser sessions
 * @param maxPayloadPreview  maximum number of payload bytes included in an event sent to the UI
 * @param reconnectDelay     delay between attempts to establish the initial connection
 */
@Validated
@ConfigurationProperties(prefix = "mqtt")
public record MqttViewerProperties(
        @DefaultValue("tcp://localhost:1883") @NotBlank String brokerUri,
        @DefaultValue("mqtt-event-viewer-") @NotBlank String clientId,
        String username,
        String password,
        @DefaultValue("#") @NotEmpty List<@NotBlank String> topics,
        @DefaultValue("0") @Min(0) @Max(2) int qos,
        @DefaultValue("500") @Min(1) @Max(100_000) int bufferSize,
        @DefaultValue("16384") @Min(64) int maxPayloadPreview,
        @DefaultValue("5s") Duration reconnectDelay) {
}
