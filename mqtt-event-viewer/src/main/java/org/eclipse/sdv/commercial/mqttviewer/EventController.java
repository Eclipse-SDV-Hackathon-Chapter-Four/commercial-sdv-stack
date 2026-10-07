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

import java.util.List;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter;

@RestController
@RequestMapping("/api")
public class EventController {

    public record Status(ConnectionState connection, long receivedCount, int bufferedCount, int bufferSize,
            int browserSessions) {
    }

    private final EventHub hub;
    private final MqttViewerProperties properties;

    public EventController(EventHub hub, MqttViewerProperties properties) {
        this.hub = hub;
        this.properties = properties;
    }

    /** Returns the most recent events, oldest first. */
    @GetMapping("/events")
    public List<MqttEvent> events(@RequestParam(defaultValue = "100") int limit) {
        return hub.recent(limit);
    }

    /**
     * Streams events as Server-Sent Events. The stream starts with a {@code status} event and a
     * {@code reset} event containing the buffered history, followed by one {@code mqtt} event per message.
     */
    @GetMapping(path = "/events/stream", produces = MediaType.TEXT_EVENT_STREAM_VALUE)
    public SseEmitter stream() {
        return hub.subscribe();
    }

    @DeleteMapping("/events")
    public ResponseEntity<Void> clear() {
        hub.clear();
        return ResponseEntity.noContent().build();
    }

    @GetMapping("/status")
    public Status status() {
        return new Status(hub.connectionState(), hub.receivedCount(), hub.bufferedCount(), properties.bufferSize(),
                hub.sessionCount());
    }
}
