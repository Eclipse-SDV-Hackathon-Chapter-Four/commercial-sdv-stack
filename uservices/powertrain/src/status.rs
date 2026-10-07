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
//! clients without uProtocol support can follow mode changes.

use std::time::Duration;

use common::powertrain::ModeMessage;
use log::{debug, info, warn};
use paho_mqtt as mqtt;
use tokio::sync::mpsc;

const CLIENT_ID: &str = "powertrain-mode-controller-status";
// bounded so updates don't pile up while the broker is unreachable
const QUEUE_SIZE: usize = 4;

pub(crate) struct StatusPublisherConfig {
    pub broker_uri: String,
    pub topic: String,
    pub username: Option<String>,
    pub password: Option<String>,
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
            while let Some(mode_message) = receiver.recv().await {
                if !client.is_connected() {
                    if let Err(e) = client.connect(connect_options.clone()).await {
                        warn!("Cannot connect to status MQTT broker: {e}");
                        continue;
                    }
                }
                let payload = match serde_json::to_vec(&mode_message) {
                    Ok(payload) => payload,
                    Err(e) => {
                        warn!("Failed to serialize powertrain mode: {e}");
                        continue;
                    }
                };
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
