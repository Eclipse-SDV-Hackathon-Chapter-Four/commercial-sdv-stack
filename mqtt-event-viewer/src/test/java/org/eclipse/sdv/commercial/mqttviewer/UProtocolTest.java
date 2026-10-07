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
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.time.Duration;
import java.util.List;
import org.eclipse.uprotocol.communication.UPayload;
import org.eclipse.uprotocol.transport.builder.UMessageBuilder;
import org.eclipse.uprotocol.uri.serializer.UriSerializer;
import org.eclipse.uprotocol.v1.UCode;
import org.eclipse.uprotocol.v1.UPayloadFormat;
import org.eclipse.uprotocol.v1.UUri;
import org.junit.jupiter.api.Test;

class UProtocolTest {

    private static final UUri FMS = UriSerializer.deserialize("up://backend/103AA/1/0");
    private static final UUri SET_MODE = UriSerializer.deserialize("up://vehicle/10301/1/2");
    private static final UUri VEHICLE_PROPERTIES = UriSerializer.deserialize("up://vehicle/10302/1/8000");

    @Test
    void topicsFollowTheInVehicleMappingOfTheMqtt5Transport() {
        var request = UMessageBuilder.request(FMS, SET_MODE, 2000).build();
        assertEquals("backend/3AA/1/1/0/vehicle/301/1/1/2", UProtocolTopics.of(request.getAttributes(), "x"));

        var event = UMessageBuilder.publish(VEHICLE_PROPERTIES).build();
        assertEquals("vehicle/302/1/1/8000", UProtocolTopics.of(event.getAttributes(), "x"));

        var local = UUri.newBuilder().setUeId(0x1_0302).setUeVersionMajor(1).setResourceId(0x8000).build();
        assertEquals("fallback/302/1/1/8000", UProtocolTopics.segments(local, "fallback"));
    }

    @Test
    void attributesAreMappedAndTokensAreRedacted() {
        var request = UMessageBuilder.request(FMS, SET_MODE, 2000)
                .withToken("secret-token")
                .build(UPayload.pack(com.google.protobuf.ByteString.copyFromUtf8("{}"),
                        UPayloadFormat.UPAYLOAD_FORMAT_JSON));
        var attributes = UProtocolAttributes.of(request.getAttributes(), UProtocolAttributes.Direction.SENT);

        assertEquals(UProtocolAttributes.Direction.SENT, attributes.direction());
        assertEquals("request", attributes.type());
        assertEquals("//backend/103AA/1/0", attributes.source());
        assertEquals("//vehicle/10301/1/2", attributes.sink());
        assertEquals("CS4", attributes.priority());
        assertEquals(2000, attributes.ttl());
        assertEquals("JSON", attributes.payloadFormat());
        assertNotNull(attributes.id());
        assertNotNull(attributes.createdAt());
        assertEquals("(redacted, 12 characters)", attributes.token());
        assertFalse(attributes.toString().contains("secret-token"));

        var response = UMessageBuilder.response(request.getAttributes())
                .withCommStatus(UCode.UNAUTHENTICATED)
                .build();
        var responseAttributes = UProtocolAttributes.of(response.getAttributes(),
                UProtocolAttributes.Direction.RECEIVED);
        assertEquals("response", responseAttributes.type());
        assertEquals("UNAUTHENTICATED", responseAttributes.commStatus());
        assertEquals(attributes.id(), responseAttributes.reqId());
        assertNull(responseAttributes.token());
    }

    @Test
    void eventsForUMessagesContainTheDecodedAttributes() {
        var message = UMessageBuilder.publish(VEHICLE_PROPERTIES)
                .build(UPayload.pack(com.google.protobuf.ByteString.copyFromUtf8("{\"a\":1}"),
                        UPayloadFormat.UPAYLOAD_FORMAT_JSON));
        var event = MqttEvent.ofUMessage(1, java.time.Instant.EPOCH, "vehicle/302/1/1/8000",
                message.getPayload().toByteArray(),
                UProtocolAttributes.of(message.getAttributes(), UProtocolAttributes.Direction.RECEIVED), 1024);
        assertEquals(MqttEvent.PayloadFormat.JSON, event.payloadFormat());
        assertEquals("publish", event.uprotocol().type());
        assertEquals("CS1", event.uprotocol().priority());
        assertEquals(List.of(), event.userProperties());
    }

    @Test
    void localUriMustIdentifyAnEntity() {
        assertEquals("backend", UProtocolClient.parseLocalUri("up://backend/103AB/1/0").getAuthorityName());
        assertThrows(IllegalArgumentException.class, () -> UProtocolClient.parseLocalUri("up:///103AB/1/0"));
        assertThrows(IllegalArgumentException.class, () -> UProtocolClient.parseLocalUri("up://backend/103AB/1/1"));
        assertThrows(IllegalArgumentException.class, () -> UProtocolClient.parseLocalUri("up://*/103AB/1/0"));
        assertThrows(IllegalArgumentException.class, () -> UProtocolClient.parseLocalUri("no uri"));
    }

    @Test
    void duplicateDeliveriesAreDetected() {
        var client = new UProtocolClient(
                new MqttViewerProperties("tcp://127.0.0.1:1", "test", null, null, List.of("#"), 0, 10, 1024,
                        Duration.ofHours(1)),
                new UProtocolProperties(true, "up://backend/103AB/1/0", true, Duration.ofSeconds(1)),
                null);
        var first = UMessageBuilder.publish(VEHICLE_PROPERTIES).build();
        var second = UMessageBuilder.publish(VEHICLE_PROPERTIES).build();
        assertTrue(client.isFirstDelivery(first));
        assertFalse(client.isFirstDelivery(first));
        assertTrue(client.isFirstDelivery(second));
    }

    @Test
    void rpcMethodsAreValidated() {
        assertEquals(1, UProtocolController.parseMethod("up://vehicle/10301/1/1").getResourceId());
        assertThrows(IllegalArgumentException.class, () -> UProtocolController.parseMethod(null));
        assertThrows(IllegalArgumentException.class, () -> UProtocolController.parseMethod("up://vehicle/10301/1/0"));
        assertThrows(IllegalArgumentException.class,
                () -> UProtocolController.parseMethod("up://vehicle/10301/1/8000"));
        assertThrows(IllegalArgumentException.class, () -> UProtocolController.parseMethod("up://*/10301/1/1"));
    }

    @Test
    void rpcPayloadsAreConverted() {
        assertEquals(UPayload.EMPTY, UProtocolController.toPayload(null, null));
        assertEquals(UPayloadFormat.UPAYLOAD_FORMAT_JSON, UProtocolController.toPayload("{}", null).format());
        assertEquals(UPayloadFormat.UPAYLOAD_FORMAT_TEXT, UProtocolController.toPayload("x", "text").format());
        assertThrows(IllegalArgumentException.class, () -> UProtocolController.toPayload("x", "PROTOBUF"));
    }
}
