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

import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.Arrays;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import org.eclipse.paho.mqttv5.client.IMqttToken;
import org.eclipse.paho.mqttv5.client.MqttActionListener;
import org.eclipse.paho.mqttv5.client.MqttAsyncClient;
import org.eclipse.paho.mqttv5.client.MqttCallback;
import org.eclipse.paho.mqttv5.client.MqttConnectionOptions;
import org.eclipse.paho.mqttv5.client.MqttDisconnectResponse;
import org.eclipse.paho.mqttv5.client.persist.MemoryPersistence;
import org.eclipse.paho.mqttv5.common.MqttException;
import org.eclipse.paho.mqttv5.common.MqttMessage;
import org.eclipse.paho.mqttv5.common.packet.MqttProperties;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.context.SmartLifecycle;
import org.springframework.stereotype.Component;

/**
 * Subscribes to the configured topic filters and forwards all received messages to the {@link EventHub}.
 *
 * <p>Only active if the uProtocol mode is disabled ({@code uprotocol.enabled=false}), see {@link UProtocolClient}.
 *
 * <p>The initial connection attempt is retried until it succeeds; afterwards Paho's automatic
 * reconnect takes over. Subscriptions are (re-)established on every successful connect.
 */
@Component
@ConditionalOnProperty(prefix = "uprotocol", name = "enabled", havingValue = "false")
public class MqttSubscriber implements SmartLifecycle, MqttCallback {

    private static final Logger LOG = LoggerFactory.getLogger(MqttSubscriber.class);

    private final MqttViewerProperties properties;
    private final EventHub hub;
    private final String clientId;
    private final ScheduledExecutorService retryScheduler = Executors.newSingleThreadScheduledExecutor(r -> {
        var t = new Thread(r, "mqtt-connect-retry");
        t.setDaemon(true);
        return t;
    });

    private volatile MqttAsyncClient client;
    private volatile boolean running;

    public MqttSubscriber(MqttViewerProperties properties, EventHub hub) {
        this.properties = properties;
        this.hub = hub;
        this.clientId = properties.clientId().endsWith("-")
                ? properties.clientId() + UUID.randomUUID().toString().substring(0, 8)
                : properties.clientId();
    }

    @Override
    public void start() {
        try {
            client = new MqttAsyncClient(properties.brokerUri(), clientId, new MemoryPersistence());
        } catch (MqttException e) {
            throw new IllegalStateException("invalid MQTT configuration: " + e.getMessage(), e);
        }
        client.setCallback(this);
        running = true;
        connect();
    }

    @Override
    public void stop() {
        running = false;
        retryScheduler.shutdownNow();
        var c = client;
        if (c == null) {
            return;
        }
        try {
            if (c.isConnected()) {
                c.disconnect().waitForCompletion(2_000);
            }
        } catch (MqttException e) {
            LOG.debug("error while disconnecting from MQTT broker", e);
        } finally {
            try {
                c.close(true);
            } catch (MqttException e) {
                LOG.debug("error while closing MQTT client", e);
            }
        }
    }

    @Override
    public boolean isRunning() {
        return running;
    }

    private MqttConnectionOptions connectionOptions() {
        var options = new MqttConnectionOptions();
        options.setCleanStart(true);
        options.setAutomaticReconnect(true);
        options.setAutomaticReconnectDelay(1, 30);
        options.setConnectionTimeout(10);
        options.setKeepAliveInterval(30);
        if (properties.username() != null && !properties.username().isBlank()) {
            options.setUserName(properties.username());
        }
        if (properties.password() != null && !properties.password().isEmpty()) {
            options.setPassword(properties.password().getBytes(StandardCharsets.UTF_8));
        }
        return options;
    }

    private void connect() {
        if (!running) {
            return;
        }
        LOG.info("connecting to MQTT broker {} as {}", properties.brokerUri(), clientId);
        try {
            client.connect(connectionOptions(), null, new MqttActionListener() {
                @Override
                public void onSuccess(IMqttToken token) {
                    // subscriptions are handled in connectComplete()
                }

                @Override
                public void onFailure(IMqttToken token, Throwable e) {
                    onConnectFailure(e);
                }
            });
        } catch (MqttException e) {
            onConnectFailure(e);
        }
    }

    private void onConnectFailure(Throwable e) {
        LOG.warn("failed to connect to MQTT broker {}: {} - retrying in {}",
                properties.brokerUri(), e.getMessage(), properties.reconnectDelay());
        updateState(false, e.getMessage());
        if (running) {
            try {
                retryScheduler.schedule(this::connect, properties.reconnectDelay().toMillis(), TimeUnit.MILLISECONDS);
            } catch (RejectedExecutionException ignored) {
                // shutting down
            }
        }
    }

    @Override
    public void connectComplete(boolean reconnect, String serverUri) {
        LOG.info("{} MQTT broker {}", reconnect ? "reconnected to" : "connected to", serverUri);
        var filters = properties.topics().toArray(String[]::new);
        var qos = new int[filters.length];
        Arrays.fill(qos, properties.qos());
        try {
            client.subscribe(filters, qos, null, new MqttActionListener() {
                @Override
                public void onSuccess(IMqttToken token) {
                    LOG.info("subscribed to {}", properties.topics());
                    updateState(true, null);
                }

                @Override
                public void onFailure(IMqttToken token, Throwable e) {
                    LOG.error("failed to subscribe to {}: {}", properties.topics(), e.getMessage());
                    updateState(true, "subscription failed: " + e.getMessage());
                }
            });
        } catch (MqttException e) {
            LOG.error("failed to subscribe to {}", properties.topics(), e);
            updateState(true, "subscription failed: " + e.getMessage());
        }
    }

    @Override
    public void disconnected(MqttDisconnectResponse response) {
        var reason = response.getReasonString() != null
                ? response.getReasonString()
                : response.getException() != null ? response.getException().getMessage() : "connection lost";
        LOG.warn("disconnected from MQTT broker: {}", reason);
        updateState(false, reason);
    }

    @Override
    public void mqttErrorOccurred(MqttException exception) {
        LOG.warn("MQTT error: {}", exception.getMessage());
    }

    @Override
    public void messageArrived(String topic, MqttMessage message) {
        var props = toProperties(message.getProperties());
        var event = hub.publish(id -> MqttEvent.of(id, Instant.now(), topic, message.getQos(), message.isRetained(),
                message.isDuplicate(), message.getPayload(), props, properties.maxPayloadPreview()));
        LOG.debug("received message #{} on {} ({} bytes)", event.id(), topic, event.payloadSize());
    }

    @Override
    public void deliveryComplete(IMqttToken token) {
        // this client never publishes
    }

    @Override
    public void authPacketArrived(int reasonCode, MqttProperties properties) {
        // enhanced authentication is not used
    }

    private static MqttEvent.Properties toProperties(MqttProperties p) {
        if (p == null) {
            return MqttEvent.Properties.NONE;
        }
        var userProps = p.getUserProperties() == null
                ? List.<MqttEvent.UserProperty>of()
                : p.getUserProperties().stream()
                        .map(u -> new MqttEvent.UserProperty(u.getKey(), u.getValue()))
                        .toList();
        return new MqttEvent.Properties(p.getContentType(), p.getResponseTopic(), p.getCorrelationData(),
                p.getMessageExpiryInterval(), userProps);
    }

    private void updateState(boolean connected, String error) {
        hub.updateConnectionState(new ConnectionState(connected, properties.brokerUri(), clientId,
                properties.topics(), error, Instant.now()));
    }
}
