/********************************************************************************
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
 ********************************************************************************/

// AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16

//! Receives the AZ3166 telemetry as uProtocol events via MQTT 5 (up-transport-mqtt5).

use std::{sync::Arc, time::Duration};

use log::{debug, info, warn};
use tokio::sync::mpsc;
use up_rust::{UListener, UMessage, UTransport, UUri};
use up_transport_mqtt5::{Mqtt5Transport, Mqtt5TransportOptions, MqttClientOptions, TransportMode};

// only used for URIs without an authority
const LOCAL_AUTHORITY: &str = "vehicle";
const QUEUE_SIZE: usize = 16;

struct TelemetryForwarder(mpsc::Sender<String>);

#[async_trait::async_trait]
impl UListener for TelemetryForwarder {
    async fn on_receive(&self, message: UMessage) {
        let Some(payload) = message.payload else {
            debug!("Ignoring uProtocol telemetry without payload");
            return;
        };
        match String::from_utf8(payload.to_vec()) {
            Ok(text) => {
                if self.0.try_send(text).is_err() {
                    debug!("Telemetry queue full, dropping uProtocol telemetry");
                }
            }
            Err(_) => warn!("Ignoring uProtocol telemetry that is not UTF-8 text"),
        }
    }
}

/// Connects to the broker in the background (retrying until it succeeds) and returns the payloads
/// of the events published on `topic`.
pub(crate) fn subscribe(options: MqttClientOptions, topic: UUri) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel(QUEUE_SIZE);
    tokio::spawn(async move {
        let transport_options = Mqtt5TransportOptions {
            mqtt_client_options: options,
            mode: TransportMode::InVehicle,
            ..Default::default()
        };
        let transport = match Mqtt5Transport::new(transport_options, LOCAL_AUTHORITY).await {
            Ok(transport) => transport,
            Err(e) => {
                warn!("Cannot create uProtocol MQTT 5 transport: {e}");
                return;
            }
        };
        let mut delay = Duration::from_secs(1);
        loop {
            let registered = match transport.connect().await {
                Ok(()) => {
                    transport
                        .register_listener(&topic, None, Arc::new(TelemetryForwarder(sender.clone())))
                        .await
                }
                Err(e) => Err(e),
            };
            match registered {
                Ok(()) => break,
                Err(e) => warn!("Cannot subscribe to uProtocol telemetry: {e}"),
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(30));
        }
        info!("Subscribed to uProtocol telemetry {}", topic.to_uri(true));
        // the transport reconnects and resubscribes by itself, it only has to be kept alive
        std::future::pending::<()>().await;
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use up_rust::{UMessageBuilder, UPayloadFormat};

    #[tokio::test]
    async fn forwards_text_payload() {
        let (sender, mut receiver) = mpsc::channel(1);
        let message = UMessageBuilder::publish("up://az3166/AB/1/8001".parse().unwrap())
            .build_with_payload("Temperature: 28.5", UPayloadFormat::UPAYLOAD_FORMAT_TEXT)
            .unwrap();
        TelemetryForwarder(sender).on_receive(message).await;
        assert_eq!(receiver.recv().await.as_deref(), Some("Temperature: 28.5"));
    }
}
