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

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import org.junit.jupiter.api.Test;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.context.ApplicationContext;

/** Verifies that the viewer falls back to subscribing to raw MQTT topics if the uProtocol mode is disabled. */
@SpringBootTest(webEnvironment = SpringBootTest.WebEnvironment.RANDOM_PORT, properties = {
        "mqtt.broker-uri=tcp://127.0.0.1:1",
        "mqtt.reconnect-delay=1h",
        "uprotocol.enabled=false",
})
class RawMqttModeTest {

    @Value("${local.server.port}")
    int port;

    @Autowired
    ApplicationContext context;

    @Test
    void rawMqttSubscriberIsUsed() throws Exception {
        assertEquals(1, context.getBeanNamesForType(MqttSubscriber.class).length);
        assertEquals(0, context.getBeanNamesForType(UProtocolClient.class).length);

        var http = HttpClient.newBuilder()
                .proxy(HttpClient.Builder.NO_PROXY)
                .connectTimeout(Duration.ofSeconds(5))
                .build();
        var info = http.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + "/api/uprotocol")).build(),
                HttpResponse.BodyHandlers.ofString());
        assertEquals(404, info.statusCode());
        var status = http.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + "/api/status")).build(),
                HttpResponse.BodyHandlers.ofString());
        assertTrue(status.body().contains("\"topics\":[\"#\"]"), status.body());
    }
}
