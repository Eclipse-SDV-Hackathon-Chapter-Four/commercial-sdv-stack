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

import com.google.protobuf.ByteString;
import java.time.Duration;
import java.util.HexFormat;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import org.eclipse.uprotocol.communication.UPayload;
import org.eclipse.uprotocol.communication.UStatusException;
import org.eclipse.uprotocol.uri.serializer.UriSerializer;
import org.eclipse.uprotocol.uri.validator.UriValidator;
import org.eclipse.uprotocol.v1.UCode;
import org.eclipse.uprotocol.v1.UPayloadFormat;
import org.eclipse.uprotocol.v1.UUri;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

/** HTTP API for the viewer's uProtocol endpoint. */
@RestController
@RequestMapping("/api/uprotocol")
@ConditionalOnProperty(prefix = "uprotocol", name = "enabled", havingValue = "true", matchIfMissing = true)
public class UProtocolController {

    static final long MAX_TIMEOUT_MILLIS = 60_000;

    /** The viewer's uProtocol identity and capabilities. */
    public record Info(String localUri, boolean rpcEnabled, long rpcTimeoutMillis) {
    }

    /**
     * A request to invoke a service operation.
     *
     * @param method        URI of the operation, e.g. {@code up://vehicle/10301/1/1}
     * @param payload       optional request payload
     * @param payloadFormat {@code JSON} (default) or {@code TEXT}
     * @param token         optional access token to include in the request
     * @param timeoutMillis optional time to wait for the response
     */
    public record RpcRequest(String method, String payload, String payloadFormat, String token, Long timeoutMillis) {
    }

    /**
     * The outcome of invoking a service operation.
     *
     * @param status        the uProtocol status code, {@code OK} if the operation succeeded
     * @param message       an error message, if the operation failed
     * @param durationMillis time it took to receive the response
     * @param payloadFormat the format of the response payload as indicated by the service
     * @param payloadText   the response payload, if it is printable UTF-8
     * @param payloadHex    the response payload, if it is binary
     */
    public record RpcResponse(String status, String message, Long durationMillis, String payloadFormat,
            String payloadText, String payloadHex) {

        static RpcResponse error(UCode code, String message) {
            return new RpcResponse(code.name(), message, null, null, null, null);
        }
    }

    private final UProtocolClient client;

    public UProtocolController(UProtocolClient client) {
        this.client = client;
    }

    @GetMapping
    public Info info() {
        return new Info(UriSerializer.serialize(client.localUri()), client.rpcEnabled(),
                client.rpcTimeout().toMillis());
    }

    /**
     * Invokes a uProtocol service operation and returns its outcome. Errors reported by the service
     * or the transport are returned with HTTP status 200 and the corresponding uProtocol status code.
     */
    @PostMapping("/rpc")
    public CompletableFuture<ResponseEntity<RpcResponse>> invoke(@RequestBody RpcRequest request) {
        if (!client.rpcEnabled()) {
            return CompletableFuture.completedFuture(ResponseEntity.status(HttpStatus.FORBIDDEN)
                    .body(RpcResponse.error(UCode.PERMISSION_DENIED, "invoking service operations is disabled")));
        }
        final UUri method;
        final UPayload payload;
        final Duration timeout;
        try {
            method = parseMethod(request.method());
            payload = toPayload(request.payload(), request.payloadFormat());
            timeout = toTimeout(request.timeoutMillis());
        } catch (IllegalArgumentException e) {
            return CompletableFuture.completedFuture(ResponseEntity.badRequest()
                    .body(RpcResponse.error(UCode.INVALID_ARGUMENT, e.getMessage())));
        }

        var start = System.nanoTime();
        return client.invokeMethod(method, payload, request.token(), timeout)
                .handle((response, error) -> {
                    var duration = Duration.ofNanos(System.nanoTime() - start).toMillis();
                    return ResponseEntity.ok(error == null
                            ? success(response, duration)
                            : failure(error, duration));
                })
                .toCompletableFuture();
    }

    static UUri parseMethod(String method) {
        if (method == null || method.isBlank()) {
            throw new IllegalArgumentException("method URI is required");
        }
        final UUri uri;
        try {
            uri = UriSerializer.deserialize(method.strip());
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException("invalid method URI: " + e.getMessage(), e);
        }
        if (UriValidator.hasWildcard(uri) || !UriValidator.isRpcMethod(uri)) {
            throw new IllegalArgumentException(
                    "method URI must not contain wildcards and must have a resource ID in range [0x0001, 0x7FFF]");
        }
        return uri;
    }

    static UPayload toPayload(String payload, String format) {
        if (payload == null || payload.isEmpty()) {
            return UPayload.EMPTY;
        }
        var payloadFormat = switch (format == null ? "JSON" : format.strip().toUpperCase()) {
            case "JSON" -> UPayloadFormat.UPAYLOAD_FORMAT_JSON;
            case "TEXT" -> UPayloadFormat.UPAYLOAD_FORMAT_TEXT;
            default -> throw new IllegalArgumentException("unsupported payload format, use JSON or TEXT");
        };
        return UPayload.pack(ByteString.copyFromUtf8(payload), payloadFormat);
    }

    Duration toTimeout(Long timeoutMillis) {
        if (timeoutMillis == null) {
            return client.rpcTimeout();
        }
        if (timeoutMillis < 1 || timeoutMillis > MAX_TIMEOUT_MILLIS) {
            throw new IllegalArgumentException("timeout must be in range [1, " + MAX_TIMEOUT_MILLIS + "] ms");
        }
        return Duration.ofMillis(timeoutMillis);
    }

    private static RpcResponse success(UPayload payload, long duration) {
        var data = payload == null ? new byte[0] : payload.data().toByteArray();
        var format = payload == null || payload.format() == UPayloadFormat.UPAYLOAD_FORMAT_UNSPECIFIED
                ? null
                : payload.format().name().replace("UPAYLOAD_FORMAT_", "");
        var text = MqttEvent.decodePrintableUtf8(data);
        return new RpcResponse(UCode.OK.name(), null, duration, format,
                text.orElse(null),
                text.isPresent() ? null : HexFormat.ofDelimiter(" ").formatHex(data));
    }

    private static RpcResponse failure(Throwable error, long duration) {
        var cause = error instanceof CompletionException && error.getCause() != null ? error.getCause() : error;
        var code = cause instanceof UStatusException e ? e.getCode() : UCode.INTERNAL;
        return new RpcResponse(code.name(), cause.getMessage(), duration, null, null, null);
    }
}
