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

import java.time.Instant;
import org.eclipse.uprotocol.uri.serializer.UriSerializer;
import org.eclipse.uprotocol.uuid.factory.UuidUtils;
import org.eclipse.uprotocol.uuid.serializer.UuidSerializer;
import org.eclipse.uprotocol.v1.UAttributes;
import org.eclipse.uprotocol.v1.UPayloadFormat;
import org.eclipse.uprotocol.v1.UPriority;

/**
 * The attributes of a uProtocol message as presented to the UI.
 *
 * <p>Access tokens are never exposed, only their presence and length.
 */
public record UProtocolAttributes(
        Direction direction,
        String type,
        String id,
        Instant createdAt,
        String source,
        String sink,
        String priority,
        Integer ttl,
        Integer permissionLevel,
        String commStatus,
        String reqId,
        String token,
        String traceparent,
        String payloadFormat) {

    /** Whether the viewer received or sent a message. */
    public enum Direction { RECEIVED, SENT }

    static UProtocolAttributes of(UAttributes a, Direction direction) {
        String id = null;
        Instant createdAt = null;
        if (a.hasId()) {
            id = UuidSerializer.serialize(a.getId());
            if (UuidUtils.isUProtocol(a.getId())) {
                createdAt = Instant.ofEpochMilli(UuidUtils.getTimestamp(a.getId()));
            }
        }
        return new UProtocolAttributes(
                direction,
                messageType(a),
                id,
                createdAt,
                a.hasSource() ? UriSerializer.serialize(a.getSource()) : null,
                a.hasSink() ? UriSerializer.serialize(a.getSink()) : null,
                priority(a.getPriority()),
                a.hasTtl() ? a.getTtl() : null,
                a.hasPermissionLevel() ? a.getPermissionLevel() : null,
                a.hasCommstatus() ? a.getCommstatus().name() : null,
                a.hasReqid() ? UuidSerializer.serialize(a.getReqid()) : null,
                a.hasToken() ? "(redacted, %d characters)".formatted(a.getToken().length()) : null,
                a.hasTraceparent() ? a.getTraceparent() : null,
                a.getPayloadFormat() == UPayloadFormat.UPAYLOAD_FORMAT_UNSPECIFIED
                        || a.getPayloadFormat() == UPayloadFormat.UNRECOGNIZED
                        ? null
                        : a.getPayloadFormat().name().replace("UPAYLOAD_FORMAT_", ""));
    }

    private static String priority(UPriority priority) {
        return switch (priority) {
            // messages without a priority belong to class CS1 by default
            case UPRIORITY_UNSPECIFIED -> "CS1";
            case UNRECOGNIZED -> null;
            default -> priority.name().replace("UPRIORITY_", "");
        };
    }

    private static String messageType(UAttributes a) {
        return switch (a.getType()) {
            case UMESSAGE_TYPE_PUBLISH -> "publish";
            case UMESSAGE_TYPE_REQUEST -> "request";
            case UMESSAGE_TYPE_RESPONSE -> "response";
            case UMESSAGE_TYPE_NOTIFICATION -> "notification";
            default -> "unspecified";
        };
    }
}
