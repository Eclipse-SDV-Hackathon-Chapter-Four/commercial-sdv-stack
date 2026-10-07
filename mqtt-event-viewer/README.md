<!--
SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Apache License Version 2.0 which is available at
https://www.apache.org/licenses/LICENSE-2.0

SPDX-License-Identifier: Apache-2.0
-->
<!-- Portions of this file were generated with AI assistance (GitHub Copilot). -->

# MQTT Event Viewer

A small Spring Boot web application that connects to the stack's Mosquitto broker and shows the exchanged messages live in the browser.

By default, the viewer is a uProtocol entity (`up://backend/103AB/1/0`) that uses [Eclipse uProtocol](https://github.com/eclipse-uprotocol)'s MQTT 5 transport, just like the stack's uServices:

* It receives **all** uProtocol messages (publish, request, response and notification) via wildcard listeners and shows their decoded attributes (type, source, sink, priority, TTL, request ID, commstatus …).
  Access tokens are never shown, only their length.
* It can **invoke service operations** (RPC), e.g. the powertrain mode controller's `GetCurrentMode`, from the UI or via the [HTTP API](#http-api).
  The requests it sends are shown with a `sent` tag.

Set `UPROTOCOL_ENABLED=false` to subscribe to raw MQTT topic filters instead, e.g. to also see messages that are not uProtocol messages.
In this mode, the uProtocol attributes are shown as the MQTT 5 user properties that carry them.

Common features:

* Messages are pushed to the browser via Server-Sent Events. New browser sessions first receive the most recent messages (500 by default).
* Payloads are detected as JSON (pretty printed), text, or binary (hex and Base64, e.g. protobuf encoded uProtocol payloads).
* Filter by topic (substring or MQTT wildcards like `vehicle/+/#`), search payloads and attributes, and pause or clear the view.

## How uProtocol over MQTT Works

The uServices (`fms`, `vehicle-properties`, `powertrain-mode-controller`) use [up-rust](https://github.com/eclipse-uprotocol/up-rust) with the [MQTT 5 transport](https://github.com/eclipse-uprotocol/up-transport-mqtt5-rust) (`command: ["mqtt5"]` in the [Docker Compose file](../docker-compose.yaml)).
The viewer uses their Java counterparts, [up-java](https://github.com/eclipse-uprotocol/up-java) and [up-transport-mqtt5-java](https://github.com/eclipse-uprotocol/up-transport-mqtt5-java).
Both implement the [uProtocol MQTT 5 binding](https://github.com/eclipse-uprotocol/up-spec/blob/main/up-l1/mqtt_5.adoc):

* Every uEntity has a URI (`UP_LOCAL_ADDRESS`), e.g. `up://vehicle/10301/1/0` for the powertrain mode controller.
* A message is published to the topic `{authority}/{ue_type}/{ue_instance}/{ue_version}/{resource}` of its source (all numbers in hex), followed by the same segments for its sink, if any.
  For example, the FMS invoking `SetCurrentMode` (`up://vehicle/10301/1/2`) publishes the request to `backend/3AA/1/1/0/vehicle/301/1/1/2`.
* The message attributes are carried in MQTT 5 user properties (`uP`, `1` … `11`), the request ID of a response in the correlation data.

The powertrain mode controller only accepts requests that contain a JWT-SVID issued by SPIRE for audience `powertrain.mode-control` in the `token` attribute and whose SPIFFE ID is authorized in [authorization-data.json](../config/powertrain-mode-controller/authorization-data.json).
Requests sent by the viewer without such a token are answered with `UNAUTHENTICATED`.

## Architecture

```text
                    ┌─ UProtocolClient (up-java + up-transport-mqtt5-java, HiveMQ client) ─┐
Mosquitto ◀─MQTT 5─▶┤                                                                       ├─▶ EventHub (ring buffer) ──SSE──▶ browser (static/index.html)
                    └─ MqttSubscriber (Eclipse Paho), if UPROTOCOL_ENABLED=false ───────────┘        │
                                                                                                      └── REST: /api/events, /api/status, /api/uprotocol
```

| Class | Responsibility |
|-------|----------------|
| [`UProtocolClient`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/UProtocolClient.java) | Connects as a uEntity (retrying until the broker is available), registers the wildcard listeners, invokes service operations and forwards received and sent messages |
| [`UProtocolController`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/UProtocolController.java) | uProtocol REST endpoints |
| [`MqttSubscriber`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/MqttSubscriber.java) | Raw MQTT mode: subscribes to the configured topic filters and forwards messages |
| [`MqttEvent`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/MqttEvent.java) | UI representation of a message, including payload format detection and the decoded [uProtocol attributes](src/main/java/org/eclipse/sdv/commercial/mqttviewer/UProtocolAttributes.java) |
| [`EventHub`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/EventHub.java) | Keeps the most recent messages and fans them out to all browser sessions |
| [`EventController`](src/main/java/org/eclipse/sdv/commercial/mqttviewer/EventController.java) | REST and SSE endpoints |

## Run It

### With Docker Compose (Recommended)

From the repository root, add the `mqtt-viewer` profile to the profiles you are already using, for example:

```bash
docker compose --profile infra --profile powertrain --profile mqtt-viewer up -d --build
# only (re)build and (re)start the viewer, e.g. after changing it
docker compose --profile mqtt-viewer up -d --build mqtt-event-viewer
```

Then open http://localhost:8090. The badge in the header shows the broker connection; hover over it to see the viewer's uProtocol URI and listeners.

The first image build takes a few minutes because it builds up-transport-mqtt5-java from source.

### Locally

Requires Java 21+ and Maven. The broker must be reachable at `localhost:1883`, which is the case when the stack is running.

up-transport-mqtt5-java has not been released to Maven Central yet. Build it and install it into your local Maven repository once (the Docker build does this automatically):

```bash
cd mqtt-event-viewer
./install-up-transport-mqtt5.sh
mvn spring-boot:run
# or
mvn package && java -jar target/mqtt-event-viewer.jar
```

> [!WARNING]
> On macOS and Windows, the stack runs in Docker Desktop's VM whose clock is usually a few milliseconds ahead of the host's clock.
> A viewer running directly on the host then drops the requests and responses of the other uEntities, see [Known Limitations](#known-limitations).
> Use Docker Compose instead, or `UPROTOCOL_ENABLED=false` to see the raw MQTT messages.

## Invoking Service Operations

Use the _Invoke RPC_ button in the header or the [HTTP API](#http-api). The powertrain mode controller (`up://vehicle/10301/1/0`) offers:

| Operation | Method URI | Request payload | Response payload |
|-----------|------------|-----------------|------------------|
| `GetCurrentMode` | `up://vehicle/10301/1/1` | – | `{"Mode":"Economy"}` |
| `SetCurrentMode` | `up://vehicle/10301/1/2` | `{"Mode":"Economy"}` (`Performance`, `Economy`, `EvOnly`, `ForcedCharging`) | – |

The powertrain mode controller requires a JWT-SVID for audience `powertrain.mode-control` whose SPIFFE ID is authorized in [authorization-data.json](../config/powertrain-mode-controller/authorization-data.json); without a token it answers `UNAUTHENTICATED`, with a token for a SPIFFE ID that is not authorized `PERMISSION_DENIED`.
The viewer does not fetch tokens itself. For testing, mint one with the SPIRE server's admin API, like the [integration tests](../tests/README.md) do, and paste it into the _Access token_ field:

```bash
# from the repository root, with the stack running; the token is valid for 5 minutes at most
docker compose exec -T spire-server /opt/spire/bin/spire-server jwt mint \
  -socketPath /run/spire/server/private/api.sock \
  -spiffeID spiffe://sdv.eclipse.org/backend/fms -audience powertrain.mode-control -ttl 5m | grep '^eyJ'
```

Minting a token for the FMS's SPIFFE ID impersonates the FMS, which is fine for a demo but bypasses workload attestation.
To give the viewer an identity of its own, add an entry for its image (e.g. `spiffe://sdv.eclipse.org/backend/mqtt-event-viewer`) to [register_workloads.sh](../scripts/register_workloads.sh), authorize that ID for method `1` (read only) in [authorization-data.json](../config/powertrain-mode-controller/authorization-data.json) and let the viewer fetch its tokens from the backend SPIRE agent (not implemented yet).

## Using It with the i.MX95 Boards

The two-board deployment lives in `deploy/imx95` on the `integrated_demo_branch` and `feature/imx95-distributed` branches: the Mac runs the backend including the uProtocol broker, board A (`192.168.2.10`) runs the CDA and the powertrain mode controller, board B runs the ecu-sim.
The powertrain mode controller on board A connects to the Mosquitto broker on the Mac (`MQTT_BROKER_URI=mqtt://${MAC_IP}:1883`), so the viewer runs **on the Mac** and needs no board specific configuration.

1. Start the backend with `deploy/imx95/backend.sh` and the boards with `deploy/imx95/deploy.sh` as usual.
2. Start the viewer using the same Compose files and profiles that `backend.sh` uses:

   ```bash
   docker compose -f docker-compose.yaml -f deploy/imx95/docker-compose.mac.yaml \
     --profile infra --profile fw-update --profile powertrain --profile mqtt-viewer \
     up -d --build mqtt-event-viewer
   ```

3. Open http://localhost:8090. The FMS's requests and the responses of the powertrain mode controller on board A show up as `request` and `response` rows.
   `vehicle/301/…` topics are published by board A.

Board A additionally runs a separate _status_ broker (`deploy/imx95/board-a/mosquitto`) for the powertrain mode (`vehicle/powertrain/mode`), the anomaly detector (`vehicle/anomaly`) and the MXChip AZ3166 telemetry (`ThreadXAZ3166/#`).
These are plain MQTT messages, not uProtocol messages. To watch them, start a second viewer in raw MQTT mode; anonymous clients may read these topics:

```bash
docker run -d --rm --name mqtt-event-viewer-board-a -p 127.0.0.1:8091:8090 \
  -e UPROTOCOL_ENABLED=false -e MQTT_BROKER_URI=tcp://192.168.2.10:1883 \
  -e MQTT_TOPICS=vehicle/#,signals/#,ThreadXAZ3166/# \
  ghcr.io/eclipse-sdv-blueprints/commercial-sdv-stack/mqtt-event-viewer:latest
```

Then open http://localhost:8091.

Running the viewer on a board is possible but not needed. If you do, build a `linux/arm64` image (`docker buildx build --platform linux/arm64 -t ghcr.io/eclipse-sdv-blueprints/commercial-sdv-stack/mqtt-event-viewer:latest --load .`), copy it with `docker save … | ssh root@192.168.2.10 docker load` and run it with `--network host` (the board kernel lacks the iptables raw table that Docker needs for bridge networks) and `MQTT_BROKER_URI=tcp://${MAC_IP}:1883`.

> [!NOTE]
> The boards set their clocks from the NTP server that `backend.sh` runs on the Mac.
> If the requests and responses of board A do not show up in the viewer, board A's clock is ahead of the Docker VM's clock (see [Known Limitations](#known-limitations)); check the offset with `timedatectl timesync-status` on the board.

## Configuration

All settings live in [application.yaml](src/main/resources/application.yaml) and can be overridden via environment variables:

| Environment variable | Default | Description |
|----------------------|---------|-------------|
| `MQTT_BROKER_URI` | `tcp://localhost:1883` | Broker to connect to (`ssl://…` for TLS) |
| `MQTT_CLIENT_ID` | `mqtt-event-viewer-` | Client ID; a random suffix is appended if it ends with `-` |
| `MQTT_USERNAME` / `MQTT_PASSWORD` | – | Optional broker credentials |
| `MQTT_BUFFER_SIZE` | `500` | Number of messages kept in memory and replayed to new browser sessions |
| `MQTT_MAX_PAYLOAD_PREVIEW` | `16384` | Maximum number of payload bytes sent to the browser per message |
| `UPROTOCOL_ENABLED` | `true` | `true`: connect as a uProtocol entity, `false`: subscribe to the raw MQTT topic filters in `MQTT_TOPICS` |
| `UP_LOCAL_ADDRESS` | `up://backend/103AB/1/0` | The viewer's uProtocol URI; its authority is used for URIs without an authority |
| `UPROTOCOL_RPC_ENABLED` | `true` | Allow invoking service operations via the UI and HTTP API |
| `UPROTOCOL_RPC_TIMEOUT` | `5s` | Default time to wait for an RPC response |
| `MQTT_TOPICS` | `#` | Raw MQTT mode only: comma separated list of topic filters, e.g. `vehicle/#,backend/#` |
| `MQTT_QOS` | `0` | Raw MQTT mode only: QoS used for the subscriptions |
| `SERVER_PORT` | `8090` | HTTP port of the UI |

## HTTP API

| Method & path | Description |
|---------------|-------------|
| `GET /api/events?limit=100` | Most recent messages, oldest first |
| `GET /api/events/stream` | SSE stream: one `status` event (broker connection), one `reset` event (buffered messages), then one `mqtt` event per message |
| `DELETE /api/events` | Clears the buffer for all viewers |
| `GET /api/status` | Broker connection state and counters |
| `GET /api/uprotocol` | The viewer's uProtocol URI and whether RPC is enabled (404 in raw MQTT mode) |
| `POST /api/uprotocol/rpc` | Invokes a service operation, see below |

Example: query the powertrain mode controller's current mode (see [Invoking Service Operations](#invoking-service-operations) for how to get a token).

```bash
curl -X POST http://localhost:8090/api/uprotocol/rpc -H 'Content-Type: application/json' \
  -d '{"method": "up://vehicle/10301/1/1", "token": "<JWT-SVID, optional>", "timeoutMillis": 3000}'
# with a valid token:  {"status":"OK","message":null,"durationMillis":41,"payloadFormat":null,"payloadText":"{\"Mode\":\"Economy\"}",...}
# without a token:     {"status":"UNAUTHENTICATED","message":"Communication error","durationMillis":46,...}
```

The request may also contain a `payload` string and its `payloadFormat` (`JSON`, the default, or `TEXT`).
The response contains the uProtocol status code (`OK` if the operation succeeded) and the response payload.
Errors reported by the service or the transport are returned with HTTP status 200, invalid requests with 400.

## Known Limitations

* **up-transport-mqtt5-java is not released yet.** [install-up-transport-mqtt5.sh](install-up-transport-mqtt5.sh) builds a pinned commit against the released up-java 3.0.0 (its main branch depends on an up-java SNAPSHOT that no longer exists) and installs it into the local Maven repository.
  The Dockerfile runs the script; for local builds, run it once yourself.
  Spring Boot would upgrade the HiveMQ client's Netty dependency to 4.2, so the [pom.xml](pom.xml) pins Netty to the 4.1 version that HiveMQ is built against.
* **Clock skew makes messages disappear.** up-java 3.0.0 considers a message with a TTL to be expired if the sender's clock is ahead of the receiver's clock (the elapsed time becomes negative, which is compared as a huge unsigned number).
  The transport silently drops such messages, which affects requests and responses (the FMS sets a TTL of 2 s), but not the vehicle properties events (no TTL).
  This happens when running the viewer directly on a macOS or Windows host against a stack in Docker Desktop's VM, and may happen with the i.MX95 boards if board A's clock is ahead of the Mac's Docker VM.
  Running the viewer with Docker Compose next to the broker is not affected. The raw MQTT mode (`UPROTOCOL_ENABLED=false`) is never affected.
* **No token handling.** The viewer does not fetch JWT-SVIDs from SPIRE, so calls to the powertrain mode controller need a token that is pasted in, see [Invoking Service Operations](#invoking-service-operations).
  Tokens are sent as typed and never shown in the message list (only their length).
* **Payload format.** The Java and Rust transports map the payload format attribute differently (user property `12` vs. MQTT content type), so the payload format of messages sent by the uServices is not available to the viewer, and the format of the viewer's requests is not available to the uServices.
  Payloads are detected as JSON or text instead; the uServices do not check the attribute.
* **Duplicate deliveries.** The broker delivers a message once per matching subscription (e.g. RPC responses match the wildcard listener and the RPC client's listener). The viewer shows each message ID only once.
* **No notifications/subscriptions.** The viewer receives notifications and published events via wildcard listeners but does not use the uSubscription service, which the stack does not run.

## Security Note

The viewer has no authentication. It shows everything exchanged via the broker and, unless `UPROTOCOL_RPC_ENABLED=false`, lets anybody who can reach it invoke service operations on behalf of its uEntity.
In raw MQTT mode, it also shows the access tokens that uProtocol messages may carry (user property `10`).
Therefore, the Docker Compose file only publishes its port on `127.0.0.1`. Do not expose it on other interfaces.

## Tests

```bash
./install-up-transport-mqtt5.sh   # once
mvn test
```

The tests need no broker: they cover the uProtocol topic and attribute mapping, the HTTP API in both modes and the RPC request validation.
