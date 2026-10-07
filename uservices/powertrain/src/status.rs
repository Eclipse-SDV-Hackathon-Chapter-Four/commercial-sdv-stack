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

//! Publishes the current powertrain mode to a plain MQTT topic (retained), so that
//! clients without uProtocol support can follow mode changes, and optionally as a uProtocol
//! event via MQTT 5 on the same broker.

use std::time::Duration;

use common::powertrain::ModeMessage;
use log::{debug, info, warn};
use paho_mqtt as mqtt;
use tokio::sync::mpsc;
use up_rust::{UMessageBuilder, UPayloadFormat, UTransport, UUri};
use up_transport_mqtt5::{
    Mqtt5Transport, Mqtt5TransportOptions, MqttClientOptions, TransportMode,
};

const CLIENT_ID: &str = "powertrain-mode-controller-status";
const UPROTOCOL_CLIENT_ID: &str = "powertrain-mode-controller-uprotocol";
// bounded so updates don't pile up while the broker is unreachable
const QUEUE_SIZE: usize = 4;

pub(crate) struct StatusPublisherConfig {
    pub broker_uri: String,
    pub topic: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub uprotocol_topic: Option<UUri>,
}

pub(crate) struct StatusPublisher {
    sender: mpsc::Sender<ModeMessage>,
}

impl StatusPublisher {
    pub(crate) fn start(config: StatusPublisherConfig) -> Result<Self, mqtt::Error> {
        let client = mqtt::AsyncClient::new(
            mqtt::CreateOptionsBuilder::new()
                .server_uri(&config.broker_uri)
                .client_id(CLIENT_ID)
                .finalize(),
        )?;
        let mut connect_options = mqtt::ConnectOptionsBuilder::new();
        connect_options
            .connect_timeout(Duration::from_secs(2))
            .clean_session(true);
        if let Some(username) = &config.username {
            connect_options.user_name(username);
        }
        if let Some(password) = &config.password {
            connect_options.password(password);
        }
        let connect_options = connect_options.finalize();

        let (sender, mut receiver) = mpsc::channel::<ModeMessage>(QUEUE_SIZE);
        info!(
            "Publishing powertrain mode changes [broker: {}, topic: {}]",
            config.broker_uri, config.topic
        );
        tokio::spawn(async move {
            let uprotocol = match &config.uprotocol_topic {
                Some(topic) => match uprotocol_transport(&config, topic).await {
                    Ok(transport) => {
                        info!("Publishing powertrain mode changes as uProtocol events [topic: {}]", topic.to_uri(true));
                        Some((transport, topic.clone()))
                    }
                    Err(e) => {
                        warn!("Cannot create uProtocol MQTT 5 transport: {e}");
                        None
                    }
                },
                None => None,
            };
            while let Some(mode_message) = receiver.recv().await {
                let payload = match serde_json::to_vec(&mode_message) {
                    Ok(payload) => payload,
                    Err(e) => {
                        warn!("Failed to serialize powertrain mode: {e}");
                        continue;
                    }
                };
                if let Some((transport, topic)) = &uprotocol {
                    publish_event(transport, topic, payload.clone()).await;
                }
                if !client.is_connected() {
                    if let Err(e) = client.connect(connect_options.clone()).await {
                        warn!("Cannot connect to status MQTT broker: {e}");
                        continue;
                    }
                }
                let message = mqtt::MessageBuilder::new()
                    .topic(&config.topic)
                    .payload(payload)
                    .qos(1)
                    .retained(true)
                    .finalize();
                match client.publish(message).await {
                    Ok(_) => debug!("Published powertrain mode {:?}", mode_message.mode),
                    Err(e) => warn!("Failed to publish powertrain mode: {e}"),
                }
            }
        });
        Ok(Self { sender })
    }

    /// Queues the mode for publishing without delaying the caller.
    pub(crate) fn publish(&self, mode_message: ModeMessage) {
        if self.sender.try_send(mode_message).is_err() {
            debug!("Status publish queue full, dropping powertrain mode update");
        }
    }
}

async fn uprotocol_transport(
    config: &StatusPublisherConfig,
    topic: &UUri,
) -> Result<Mqtt5Transport, up_rust::UStatus> {
    let options = Mqtt5TransportOptions {
        mqtt_client_options: MqttClientOptions {
            client_id: Some(UPROTOCOL_CLIENT_ID.to_string()),
            broker_uri: config.broker_uri.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
            ..Default::default()
        },
        mode: TransportMode::InVehicle,
        ..Default::default()
    };
    Mqtt5Transport::new(options, topic.authority_name()).await
}

async fn publish_event(transport: &Mqtt5Transport, topic: &UUri, payload: Vec<u8>) {
    if !transport.is_connected() {
        if let Err(e) = transport.connect().await {
            warn!("Cannot connect uProtocol transport to status MQTT broker: {e}");
            return;
        }
    }
    let message = match UMessageBuilder::publish(topic.clone())
        .build_with_payload(payload, UPayloadFormat::UPAYLOAD_FORMAT_JSON)
    {
        Ok(message) => message,
        Err(e) => {
            warn!("Failed to create uProtocol powertrain mode event: {e}");
            return;
        }
    };
    match transport.send(message).await {
        Ok(()) => debug!("Published powertrain mode as uProtocol event"),
        Err(e) => warn!("Failed to publish powertrain mode as uProtocol event: {e}"),
    }
}
