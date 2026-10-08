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

import java.nio.ByteBuffer;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.Arrays;
import java.util.Base64;
import java.util.HexFormat;
import java.util.List;
import java.util.Optional;

/**
 * An MQTT message as presented to the UI.
 *
 * <p>Payloads that are valid, printable UTF-8 are exposed as text. All other payloads
 * (e.g. the protobuf encoded uProtocol messages) are exposed as hex and Base64.
 */
public record MqttEvent(
        long id,
        Instant receivedAt,
        String topic,
        int qos,
        boolean retained,
        boolean duplicate,
        int payloadSize,
        PayloadFormat payloadFormat,
        String payloadText,
        String payloadHex,
        String payloadBase64,
        boolean payloadTruncated,
        String contentType,
        String responseTopic,
        String correlationDataHex,
        Long messageExpiryInterval,
        List<UserProperty> userProperties,
        UProtocolAttributes uprotocol) {

    public enum PayloadFormat { EMPTY, JSON, TEXT, BINARY }

    public record UserProperty(String key, String value) {
    }

    /** The MQTT 5 properties of a message that are shown in the UI. */
    public record Properties(
            String contentType,
            String responseTopic,
            byte[] correlationData,
            Long messageExpiryInterval,
            List<UserProperty> userProperties) {

        public static final Properties NONE = new Properties(null, null, null, null, List.of());
    }

    public static MqttEvent of(
            long id,
            Instant receivedAt,
            String topic,
            int qos,
            boolean retained,
            boolean duplicate,
            byte[] payload,
            Properties properties,
            int maxPayloadPreview) {
        return of(id, receivedAt, topic, qos, retained, duplicate, payload, properties, maxPayloadPreview, null);
    }

    /** Creates an event for a uProtocol message that has been received or sent via the uProtocol transport. */
    public static MqttEvent ofUMessage(
            long id,
            Instant receivedAt,
            String topic,
            byte[] payload,
            UProtocolAttributes attributes,
            int maxPayloadPreview) {
        return of(id, receivedAt, topic, 1, false, false, payload, null, maxPayloadPreview, attributes);
    }

    private static MqttEvent of(
            long id,
            Instant receivedAt,
            String topic,
            int qos,
            boolean retained,
            boolean duplicate,
            byte[] payload,
            Properties properties,
            int maxPayloadPreview,
            UProtocolAttributes uprotocol) {

        var data = payload == null ? new byte[0] : payload;
        var props = properties == null ? Properties.NONE : properties;
        var correlationHex = props.correlationData() == null
                ? null
                : HexFormat.of().formatHex(props.correlationData());
        var userProps = props.userProperties() == null ? List.<UserProperty>of() : List.copyOf(props.userProperties());

        PayloadFormat format;
        String text = null;
        String hex = null;
        String base64 = null;
        boolean truncated = false;

        if (data.length == 0) {
            format = PayloadFormat.EMPTY;
        } else {
            var decoded = decodePrintableUtf8(data);
            if (decoded.isPresent()) {
                var full = decoded.get();
                format = looksLikeJson(full) ? PayloadFormat.JSON : PayloadFormat.TEXT;
                truncated = full.length() > maxPayloadPreview;
                text = truncated ? full.substring(0, maxPayloadPreview) : full;
            } else {
                format = PayloadFormat.BINARY;
                truncated = data.length > maxPayloadPreview;
                var preview = truncated ? Arrays.copyOf(data, maxPayloadPreview) : data;
                hex = HexFormat.ofDelimiter(" ").formatHex(preview);
                base64 = Base64.getEncoder().encodeToString(preview);
            }
        }

        return new MqttEvent(id, receivedAt, topic, qos, retained, duplicate, data.length, format,
                text, hex, base64, truncated, props.contentType(), props.responseTopic(), correlationHex,
                props.messageExpiryInterval(), userProps, uprotocol);
    }

    static Optional<String> decodePrintableUtf8(byte[] data) {
        String text;
        try {
            text = StandardCharsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(data))
                    .toString();
        } catch (CharacterCodingException e) {
            return Optional.empty();
        }
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (Character.isISOControl(c) && c != '\n' && c != '\r' && c != '\t') {
                return Optional.empty();
            }
        }
        return Optional.of(text);
    }

    private static boolean looksLikeJson(String text) {
        var trimmed = text.strip();
        return (trimmed.startsWith("{") && trimmed.endsWith("}"))
                || (trimmed.startsWith("[") && trimmed.endsWith("]"));
    }
}
