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

import com.hivemq.client.mqtt.MqttClient;
import com.hivemq.client.mqtt.lifecycle.MqttClientDisconnectedContext;
import com.hivemq.client.mqtt.mqtt5.Mqtt5AsyncClient;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.eclipse.uprotocol.communication.CallOptions;
import org.eclipse.uprotocol.communication.InMemoryRpcClient;
import org.eclipse.uprotocol.communication.UPayload;
import org.eclipse.uprotocol.communication.UStatusException;
import org.eclipse.uprotocol.mqtt.HiveMqTransportFactory;
import org.eclipse.uprotocol.mqtt.TransportMode;
import org.eclipse.uprotocol.transport.StaticUriProvider;
import org.eclipse.uprotocol.transport.UListener;
import org.eclipse.uprotocol.transport.UTransport;
import org.eclipse.uprotocol.uri.factory.UriFactory;
import org.eclipse.uprotocol.uri.serializer.UriSerializer;
import org.eclipse.uprotocol.uri.validator.UriValidator;
import org.eclipse.uprotocol.v1.UCode;
import org.eclipse.uprotocol.v1.UMessage;
import org.eclipse.uprotocol.v1.UPriority;
import org.eclipse.uprotocol.v1.UUri;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.context.SmartLifecycle;
import org.springframework.stereotype.Component;

/**
 * Connects to the broker as a uProtocol entity using Eclipse uProtocol's MQTT 5 transport.
 *
 * <p>All uProtocol messages are received via two wildcard listeners, one for published events and one for
 * requests, responses and notifications, and forwarded to the {@link EventHub}. If enabled, uProtocol
 * service operations can be invoked via {@link #invokeMethod(UUri, UPayload, String, Duration)}. Messages
 * sent by the viewer itself are forwarded to the {@link EventHub} as well.
 *
 * <p>Connection attempts are retried until they succeed. After a connection loss, the HiveMQ client
 * reconnects and re-establishes the subscriptions automatically.
 */
@Component
@ConditionalOnProperty(prefix = "uprotocol", name = "enabled", havingValue = "true", matchIfMissing = true)
public class UProtocolClient implements SmartLifecycle {

    private static final Logger LOG = LoggerFactory.getLogger(UProtocolClient.class);
    private static final String ANY_URI = UriSerializer.serialize(UriFactory.ANY);
    private static final List<String> LISTENER_FILTERS = List.of(
            "publish: source " + ANY_URI,
            "request/response/notification: source " + ANY_URI + ", sink " + ANY_URI);
    private static final long SETUP_TIMEOUT_SECONDS = 10;
    private static final int MAX_REMEMBERED_MESSAGE_IDS = 1_000;

    private final MqttViewerProperties mqttProperties;
    private final UProtocolProperties properties;
    private final EventHub hub;
    private final String clientId;
    private final UUri localUri;
    private final AtomicBoolean initialized = new AtomicBoolean();
    // the broker delivers a message once per matching subscription, e.g. RPC responses match the wildcard
    // subscription and the RPC client's subscription
    private final Map<org.eclipse.uprotocol.v1.UUID, Boolean> recentlyReceived = new LinkedHashMap<>() {
        @Override
        protected boolean removeEldestEntry(Map.Entry<org.eclipse.uprotocol.v1.UUID, Boolean> eldest) {
            return size() > MAX_REMEMBERED_MESSAGE_IDS;
        }
    };
    private final ExecutorService setupExecutor = Executors.newSingleThreadExecutor(r -> {
        var t = new Thread(r, "uprotocol-setup");
        t.setDaemon(true);
        return t;
    });

    private volatile Mqtt5AsyncClient client;
    private volatile InMemoryRpcClient rpcClient;
    private volatile boolean running;

    public UProtocolClient(MqttViewerProperties mqttProperties, UProtocolProperties properties, EventHub hub) {
        this.mqttProperties = mqttProperties;
        this.properties = properties;
        this.hub = hub;
        this.localUri = parseLocalUri(properties.localUri());
        this.clientId = mqttProperties.clientId().endsWith("-")
                ? mqttProperties.clientId() + UUID.randomUUID().toString().substring(0, 8)
                : mqttProperties.clientId();
    }

    static UUri parseLocalUri(String uri) {
        final UUri parsed;
        try {
            parsed = UriSerializer.deserialize(uri);
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException("invalid uprotocol.local-uri [" + uri + "]: " + e.getMessage(), e);
        }
        if (parsed.getAuthorityName().isBlank() || UriValidator.hasWildcard(parsed)
                || parsed.getResourceId() != 0) {
            throw new IllegalArgumentException("uprotocol.local-uri [" + uri
                    + "] must contain an authority, must not contain wildcards and must have resource ID 0");
        }
        return parsed;
    }

    public UUri localUri() {
        return localUri;
    }

    public boolean rpcEnabled() {
        return properties.rpcEnabled();
    }

    public Duration rpcTimeout() {
        return properties.rpcTimeout();
    }

    @Override
    public void start() {
        var broker = URI.create(mqttProperties.brokerUri());
        var tls = "ssl".equals(broker.getScheme()) || "mqtts".equals(broker.getScheme());
        var builder = MqttClient.builder()
                .useMqttVersion5()
                .identifier(clientId)
                .serverHost(broker.getHost())
                .serverPort(broker.getPort() > 0 ? broker.getPort() : tls ? 8883 : 1883)
                .addConnectedListener(ctx -> onConnected())
                .addDisconnectedListener(this::onDisconnected);
        if (tls) {
            builder = builder.sslWithDefaultConfig();
        }
        if (mqttProperties.username() != null && !mqttProperties.username().isBlank()) {
            var auth = builder.simpleAuth().username(mqttProperties.username());
            if (mqttProperties.password() != null && !mqttProperties.password().isEmpty()) {
                auth = auth.password(mqttProperties.password().getBytes(StandardCharsets.UTF_8));
            }
            builder = auth.applySimpleAuth();
        }
        client = builder.buildAsync();
        running = true;
        updateState(false, null);
        LOG.info("connecting to MQTT broker {} as uEntity {} (client ID {})",
                mqttProperties.brokerUri(), UriSerializer.serialize(localUri), clientId);
        client.connectWith()
                .cleanStart(true)
                .keepAlive(30)
                .send();
    }

    @Override
    public void stop() {
        running = false;
        setupExecutor.shutdownNow();
        var rpc = rpcClient;
        if (rpc != null) {
            rpc.close();
        }
        var c = client;
        if (c != null && c.getState().isConnected()) {
            try {
                c.disconnect().get(2, TimeUnit.SECONDS);
            } catch (Exception e) {
                LOG.debug("error while disconnecting from MQTT broker", e);
            }
        }
    }

    @Override
    public boolean isRunning() {
        return running;
    }

    private void onConnected() {
        LOG.info("connected to MQTT broker {}", mqttProperties.brokerUri());
        if (initialized.compareAndSet(false, true)) {
            setupExecutor.execute(this::setUpTransport);
        } else {
            // the HiveMQ client re-subscribes automatically
            updateState(true, null);
        }
    }

    private void onDisconnected(MqttClientDisconnectedContext context) {
        var reason = Optional.ofNullable(context.getCause().getMessage()).orElse("connection lost");
        updateState(false, reason);
        if (running) {
            LOG.warn("not connected to MQTT broker {}: {} - retrying in {}",
                    mqttProperties.brokerUri(), reason, mqttProperties.reconnectDelay());
            context.getReconnector()
                    .reconnect(true)
                    .delay(mqttProperties.reconnectDelay().toMillis(), TimeUnit.MILLISECONDS);
        }
    }

    private void setUpTransport() {
        try {
            var transport = new RecordingTransport(HiveMqTransportFactory.createInstance(
                    client, TransportMode.IN_VEHICLE, localUri.getAuthorityName()));
            UListener listener = message -> {
                if (isFirstDelivery(message)) {
                    record(message, UProtocolAttributes.Direction.RECEIVED);
                }
            };
            transport.registerListener(UriFactory.ANY, Optional.empty(), listener)
                    .toCompletableFuture().get(SETUP_TIMEOUT_SECONDS, TimeUnit.SECONDS);
            transport.registerListener(UriFactory.ANY, Optional.of(UriFactory.ANY), listener)
                    .toCompletableFuture().get(SETUP_TIMEOUT_SECONDS, TimeUnit.SECONDS);
            LOG.info("registered uProtocol listeners {}", LISTENER_FILTERS);
            if (properties.rpcEnabled()) {
                rpcClient = new InMemoryRpcClient(transport, StaticUriProvider.of(localUri));
            }
            updateState(true, null);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } catch (Exception e) {
            LOG.error("failed to set up uProtocol transport", e);
            initialized.set(false);
            updateState(client.getState().isConnected(), "failed to set up uProtocol transport: " + e.getMessage());
        }
    }

    /**
     * Invokes a uProtocol service operation.
     *
     * @param method  the URI of the operation to invoke
     * @param payload the request payload
     * @param token   an optional access token to include in the request
     * @param timeout the time to wait for the response
     * @return the outcome, failed with a {@link UStatusException} if the operation could not be invoked
     *         or the service returned an error
     */
    public CompletionStage<UPayload> invokeMethod(UUri method, UPayload payload, String token, Duration timeout) {
        var rpc = rpcClient;
        if (!properties.rpcEnabled()) {
            return CompletableFuture.failedFuture(
                    new UStatusException(UCode.PERMISSION_DENIED, "invoking service operations is disabled"));
        }
        if (rpc == null) {
            return CompletableFuture.failedFuture(
                    new UStatusException(UCode.UNAVAILABLE, "not connected to the MQTT broker (yet)"));
        }
        var options = new CallOptions(Math.toIntExact(timeout.toMillis()), UPriority.UPRIORITY_CS4,
                token == null || token.isBlank() ? null : token);
        return rpc.invokeMethod(method, payload, options);
    }

    boolean isFirstDelivery(UMessage message) {
        if (!message.getAttributes().hasId()) {
            return true;
        }
        synchronized (recentlyReceived) {
            return recentlyReceived.put(message.getAttributes().getId(), Boolean.TRUE) == null;
        }
    }

    private void record(UMessage message, UProtocolAttributes.Direction direction) {
        var attributes = message.getAttributes();
        var event = hub.publish(id -> MqttEvent.ofUMessage(
                id,
                Instant.now(),
                UProtocolTopics.of(attributes, localUri.getAuthorityName()),
                message.getPayload().toByteArray(),
                UProtocolAttributes.of(attributes, direction),
                mqttProperties.maxPayloadPreview()));
        LOG.debug("{} uProtocol message #{} [type: {}, source: {}]", direction, event.id(), attributes.getType(),
                UriSerializer.serialize(attributes.getSource()));
    }

    private void updateState(boolean connected, String error) {
        hub.updateConnectionState(new ConnectionState(connected, mqttProperties.brokerUri(), clientId,
                LISTENER_FILTERS, error, Instant.now()));
    }

    /** Forwards all messages successfully sent via the transport to the {@link EventHub}. */
    private final class RecordingTransport implements UTransport {

        private final UTransport delegate;

        RecordingTransport(UTransport delegate) {
            this.delegate = delegate;
        }

        @Override
        public CompletionStage<Void> send(UMessage message) {
            return delegate.send(message)
                    .thenRun(() -> record(message, UProtocolAttributes.Direction.SENT));
        }

        @Override
        public CompletionStage<Void> registerListener(UUri sourceFilter, Optional<UUri> sinkFilter,
                UListener listener) {
            return delegate.registerListener(sourceFilter, sinkFilter, listener);
        }

        @Override
        public CompletionStage<Void> unregisterListener(UUri sourceFilter, Optional<UUri> sinkFilter,
                UListener listener) {
            return delegate.unregisterListener(sourceFilter, sinkFilter, listener);
        }
    }
}
