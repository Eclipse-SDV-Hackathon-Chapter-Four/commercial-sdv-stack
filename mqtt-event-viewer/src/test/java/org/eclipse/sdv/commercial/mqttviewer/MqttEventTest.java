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
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.List;
import org.junit.jupiter.api.Test;

class MqttEventTest {

    private static MqttEvent event(byte[] payload, int maxPreview) {
        return MqttEvent.of(1, Instant.EPOCH, "a/b", 1, false, false, payload, null, maxPreview);
    }

    @Test
    void emptyPayload() {
        var ev = event(new byte[0], 100);
        assertEquals(MqttEvent.PayloadFormat.EMPTY, ev.payloadFormat());
        assertEquals(0, ev.payloadSize());
        assertNull(ev.payloadText());
        assertNull(ev.payloadHex());
    }

    @Test
    void jsonPayload() {
        var ev = event(" {\"speed\": 42}\n".getBytes(StandardCharsets.UTF_8), 100);
        assertEquals(MqttEvent.PayloadFormat.JSON, ev.payloadFormat());
        assertEquals(" {\"speed\": 42}\n", ev.payloadText());
        assertNull(ev.payloadBase64());
    }

    @Test
    void utf8TextPayload() {
        var ev = event("Grüße\tfrom the truck".getBytes(StandardCharsets.UTF_8), 100);
        assertEquals(MqttEvent.PayloadFormat.TEXT, ev.payloadFormat());
        assertEquals("Grüße\tfrom the truck", ev.payloadText());
    }

    @Test
    void protobufLikePayloadIsBinary() {
        // field 1, length-delimited, 3 bytes: valid UTF-8 but contains control characters
        var ev = event(new byte[] {0x0a, 0x03, 'a', 'b', 'c'}, 100);
        assertEquals(MqttEvent.PayloadFormat.BINARY, ev.payloadFormat());
        assertEquals("0a 03 61 62 63", ev.payloadHex());
        assertEquals("CgNhYmM=", ev.payloadBase64());
        assertNull(ev.payloadText());
    }

    @Test
    void invalidUtf8IsBinary() {
        var ev = event(new byte[] {(byte) 0xc3, (byte) 0x28}, 100);
        assertEquals(MqttEvent.PayloadFormat.BINARY, ev.payloadFormat());
    }

    @Test
    void largePayloadsAreTruncated() {
        var text = event("x".repeat(200).getBytes(StandardCharsets.UTF_8), 64);
        assertTrue(text.payloadTruncated());
        assertEquals(64, text.payloadText().length());
        assertEquals(200, text.payloadSize());

        var binary = event(new byte[200], 64);
        assertTrue(binary.payloadTruncated());
        assertEquals(64 * 3 - 1, binary.payloadHex().length());
        assertEquals(200, binary.payloadSize());

        assertFalse(event("x".getBytes(StandardCharsets.UTF_8), 64).payloadTruncated());
    }

    @Test
    void mqtt5PropertiesAreExposed() {
        var props = new MqttEvent.Properties("application/x-protobuf", "reply/topic", new byte[] {1, (byte) 0xff},
                60L, List.of(new MqttEvent.UserProperty("uP", "1")));
        var ev = MqttEvent.of(7, Instant.EPOCH, "t", 0, true, false, new byte[] {1}, props, 10);
        assertEquals("application/x-protobuf", ev.contentType());
        assertEquals("reply/topic", ev.responseTopic());
        assertEquals("01ff", ev.correlationDataHex());
        assertEquals(60L, ev.messageExpiryInterval());
        assertEquals(List.of(new MqttEvent.UserProperty("uP", "1")), ev.userProperties());
        assertTrue(ev.retained());
    }
}
