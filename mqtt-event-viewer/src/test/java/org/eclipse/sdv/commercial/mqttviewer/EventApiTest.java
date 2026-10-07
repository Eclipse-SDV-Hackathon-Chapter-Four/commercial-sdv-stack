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

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.boot.test.context.SpringBootTest;

/**
 * Exercises the REST and SSE API. No MQTT broker is needed: the subscriber keeps retrying
 * in the background while events are injected directly into the {@link EventHub}.
 */
@SpringBootTest(webEnvironment = SpringBootTest.WebEnvironment.RANDOM_PORT, properties = {
        "mqtt.broker-uri=tcp://127.0.0.1:1",
        "mqtt.buffer-size=3",
        "mqtt.reconnect-delay=1h",
})
class EventApiTest {

    @Value("${local.server.port}")
    int port;

    @Autowired
    EventHub hub;

    private final HttpClient http = HttpClient.newBuilder()
            .proxy(HttpClient.Builder.NO_PROXY)
            .connectTimeout(Duration.ofSeconds(5))
            .build();

    @BeforeEach
    void clearBuffer() {
        hub.clear();
    }

    private void publish(String topic, String payload) {
        hub.publish(id -> MqttEvent.of(id, Instant.now(), topic, 0, false, false,
                payload.getBytes(StandardCharsets.UTF_8), null, 1024));
    }

    private HttpResponse<String> get(String path) throws Exception {
        return http.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + path)).build(),
                HttpResponse.BodyHandlers.ofString());
    }

    private HttpResponse<String> post(String path, String json) throws Exception {
        return http.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + path))
                        .header("Content-Type", "application/json")
                        .POST(HttpRequest.BodyPublishers.ofString(json))
                        .build(),
                HttpResponse.BodyHandlers.ofString());
    }

    @Test
    void uProtocolEndpointIsEnabledByDefault() throws Exception {
        var info = get("/api/uprotocol");
        assertEquals(200, info.statusCode());
        assertTrue(info.body().contains("\"localUri\":\"//backend/103AB/1/0\""), info.body());
        assertTrue(info.body().contains("\"rpcEnabled\":true"), info.body());

        var status = get("/api/status");
        assertTrue(status.body().contains("request/response/notification"), status.body());
    }

    @Test
    void rpcRequestsAreValidatedAndFailWhileDisconnected() throws Exception {
        var invalid = post("/api/uprotocol/rpc", "{\"method\":\"up://vehicle/10301/1/0\"}");
        assertEquals(400, invalid.statusCode());
        assertTrue(invalid.body().contains("\"status\":\"INVALID_ARGUMENT\""), invalid.body());

        var disconnected = post("/api/uprotocol/rpc", "{\"method\":\"up://vehicle/10301/1/1\"}");
        assertEquals(200, disconnected.statusCode());
        assertTrue(disconnected.body().contains("\"status\":\"UNAVAILABLE\""), disconnected.body());
    }

    @Test
    void recentEventsAreBounded() throws Exception {
        for (int i = 0; i < 5; i++) {
            publish("vehicle/speed", "{\"v\":" + i + "}");
        }
        var response = get("/api/events");
        assertEquals(200, response.statusCode());
        assertTrue(response.body().contains("{\\\"v\\\":2}"), response.body());
        assertTrue(response.body().contains("{\\\"v\\\":4}"), response.body());
        assertTrue(!response.body().contains("{\\\"v\\\":1}"), response.body());

        var status = get("/api/status");
        assertTrue(status.body().contains("\"bufferedCount\":3"), status.body());
        assertTrue(status.body().contains("\"connected\":false"), status.body());
    }

    @Test
    void streamReplaysHistoryThenPushesLiveEvents() throws Exception {
        publish("a/history", "first");

        var request = HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + "/api/events/stream"))
                .header("Accept", "text/event-stream")
                .build();
        var response = http.send(request, HttpResponse.BodyHandlers.ofInputStream());
        assertEquals(200, response.statusCode());

        try (var reader = new BufferedReader(new InputStreamReader(response.body(), StandardCharsets.UTF_8))) {
            var events = new ArrayList<String>();
            var data = new StringBuilder();
            String line;
            boolean publishedLive = false;
            while ((line = reader.readLine()) != null) {
                if (line.startsWith("event:")) {
                    events.add(line.substring(6));
                } else if (line.startsWith("data:")) {
                    data.append(line.substring(5)).append('\n');
                } else if (line.isEmpty() && !events.isEmpty()) {
                    if (events.equals(List.of("status", "reset")) && !publishedLive) {
                        publishedLive = true;
                        publish("b/live", "second");
                    }
                    if (events.size() == 3) {
                        break;
                    }
                }
            }
            assertEquals(List.of("status", "reset", "mqtt"), events);
            var all = data.toString();
            assertTrue(all.indexOf("a/history") < all.indexOf("b/live"), all);
            assertTrue(all.contains("\"payloadText\":\"second\""), all);
        }
    }
}
