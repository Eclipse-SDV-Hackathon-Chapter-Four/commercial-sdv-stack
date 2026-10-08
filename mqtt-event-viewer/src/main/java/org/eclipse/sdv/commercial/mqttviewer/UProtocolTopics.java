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

import org.eclipse.uprotocol.v1.UAttributes;
import org.eclipse.uprotocol.v1.UUri;

/**
 * Determines the MQTT topic that a uProtocol message is exchanged on.
 *
 * <p>Mirrors the in-vehicle topic mapping of the uProtocol MQTT 5 transport:
 * {@code {authority}/{ue_type}/{ue_instance}/{ue_version}/{resource}}, all numbers in upper case hex,
 * followed by the same segments for the sink, if present.
 *
 * @see <a href="https://github.com/eclipse-uprotocol/up-spec/blob/main/up-l1/mqtt_5.adoc">uProtocol MQTT 5 spec</a>
 */
final class UProtocolTopics {

    private UProtocolTopics() {
    }

    static String of(UAttributes attributes, String fallbackAuthority) {
        var topic = new StringBuilder(segments(attributes.getSource(), fallbackAuthority));
        if (attributes.hasSink()) {
            topic.append('/').append(segments(attributes.getSink(), fallbackAuthority));
        }
        return topic.toString();
    }

    static String segments(UUri uri, String fallbackAuthority) {
        var authority = uri.getAuthorityName().isEmpty() ? fallbackAuthority : uri.getAuthorityName();
        return "%s/%X/%X/%X/%X".formatted(
                authority,
                uri.getUeId() & 0xFFFF,
                uri.getUeId() >>> 16,
                uri.getUeVersionMajor(),
                uri.getResourceId());
    }
}
