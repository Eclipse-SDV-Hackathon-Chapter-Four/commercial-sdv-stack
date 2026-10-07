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

import jakarta.annotation.PreDestroy;
import java.io.IOException;
import java.time.Instant;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import java.util.function.LongFunction;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.event.ContextClosedEvent;
import org.springframework.context.event.EventListener;
import org.springframework.http.MediaType;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Component;
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter;

/**
 * Keeps the most recent MQTT events in memory and fans them out to connected browsers
 * via Server-Sent Events.
 *
 * <p>All writes to the browsers happen on a single dispatcher thread. This keeps the
 * MQTT client's callback thread free and guarantees that every browser receives the
 * buffered history followed by all subsequent events exactly once and in order.
 */
@Component
public class EventHub {

    static final String EVENT_MQTT = "mqtt";
    static final String EVENT_STATUS = "status";
    static final String EVENT_RESET = "reset";

    private static final Logger LOG = LoggerFactory.getLogger(EventHub.class);

    private final int capacity;
    private final Object lock = new Object();
    private final Deque<MqttEvent> buffer;
    private final List<SseEmitter> emitters = new ArrayList<>();
    private final ExecutorService dispatcher = Executors.newSingleThreadExecutor(r -> {
        var t = new Thread(r, "sse-dispatcher");
        t.setDaemon(true);
        return t;
    });

    private long nextId = 1;
    private long receivedCount;
    private volatile ConnectionState connectionState;

    public EventHub(MqttViewerProperties properties) {
        this.capacity = properties.bufferSize();
        this.buffer = new ArrayDeque<>(capacity);
        this.connectionState = new ConnectionState(false, properties.brokerUri(), null, properties.topics(),
                null, Instant.now());
    }

    /** Adds a new event, created with the next sequence number, and pushes it to all browsers. */
    public MqttEvent publish(LongFunction<MqttEvent> factory) {
        synchronized (lock) {
            var event = factory.apply(nextId++);
            receivedCount++;
            if (buffer.size() == capacity) {
                buffer.removeFirst();
            }
            buffer.addLast(event);
            dispatch(() -> sendToAll(EVENT_MQTT, event, event.id()));
            return event;
        }
    }

    /** Returns up to {@code limit} of the most recent events, oldest first. */
    public List<MqttEvent> recent(int limit) {
        synchronized (lock) {
            var all = new ArrayList<>(buffer);
            return all.subList(Math.max(0, all.size() - Math.max(0, limit)), all.size());
        }
    }

    public void clear() {
        synchronized (lock) {
            buffer.clear();
            dispatch(() -> sendToAll(EVENT_RESET, List.of(), null));
        }
    }

    public long receivedCount() {
        synchronized (lock) {
            return receivedCount;
        }
    }

    public int bufferedCount() {
        synchronized (lock) {
            return buffer.size();
        }
    }

    public ConnectionState connectionState() {
        return connectionState;
    }

    public void updateConnectionState(ConnectionState state) {
        synchronized (lock) {
            connectionState = state;
            dispatch(() -> sendToAll(EVENT_STATUS, state, null));
        }
    }

    /**
     * Registers a new browser session. The session first receives the current connection
     * state and the buffered events and then all events published afterwards.
     */
    public SseEmitter subscribe() {
        var emitter = new SseEmitter(0L);
        Runnable remove = () -> dispatch(() -> emitters.remove(emitter));
        emitter.onCompletion(remove);
        emitter.onTimeout(remove);
        emitter.onError(e -> remove.run());

        synchronized (lock) {
            var history = List.copyOf(buffer);
            var state = connectionState;
            dispatch(() -> {
                try {
                    send(emitter, EVENT_STATUS, state, null);
                    send(emitter, EVENT_RESET, history, null);
                    emitters.add(emitter);
                } catch (IOException | IllegalStateException e) {
                    LOG.debug("browser disconnected before history was sent: {}", e.getMessage());
                }
            });
        }
        return emitter;
    }

    @Scheduled(fixedDelayString = "${viewer.heartbeat-interval:15s}")
    void heartbeat() {
        dispatch(() -> new ArrayList<>(emitters).forEach(emitter -> {
            try {
                emitter.send(SseEmitter.event().comment("keep-alive"));
            } catch (IOException | IllegalStateException e) {
                emitters.remove(emitter);
            }
        }));
    }

    int sessionCount() {
        try {
            return dispatcher.submit(emitters::size).get();
        } catch (Exception e) {
            return -1;
        }
    }

    /**
     * Ends all open streams as soon as the application starts shutting down. Otherwise, the
     * web server's graceful shutdown would wait for these never-ending requests to finish.
     */
    @EventListener(ContextClosedEvent.class)
    void completeStreams() {
        dispatch(() -> {
            emitters.forEach(SseEmitter::complete);
            emitters.clear();
        });
    }

    @PreDestroy
    void shutdown() {
        completeStreams();
        dispatcher.shutdown();
    }

    private void dispatch(Runnable task) {
        try {
            dispatcher.execute(task);
        } catch (RejectedExecutionException e) {
            LOG.debug("dispatcher has been shut down, dropping task");
        }
    }

    private void sendToAll(String name, Object data, Long id) {
        var it = emitters.iterator();
        while (it.hasNext()) {
            var emitter = it.next();
            try {
                send(emitter, name, data, id);
            } catch (IOException | IllegalStateException e) {
                LOG.debug("removing disconnected browser session: {}", e.getMessage());
                it.remove();
            }
        }
    }

    private static void send(SseEmitter emitter, String name, Object data, Long id) throws IOException {
        var event = SseEmitter.event().name(name).data(data, MediaType.APPLICATION_JSON);
        if (id != null) {
            event.id(Long.toString(id));
        }
        emitter.send(event);
    }
}
