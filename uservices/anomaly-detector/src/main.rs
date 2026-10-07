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

//! Detects anomalies in the MXChip AZ3166 telemetry received via MQTT and publishes the result.

use std::{path::PathBuf, time::Duration, time::SystemTime};

use clap::Parser;
use log::{debug, info, warn};
use paho_mqtt as mqtt;
use tokio::signal::unix::{SignalKind, signal};

mod model;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The URI of the MQTT broker that carries the telemetry.
    #[arg(
        long,
        value_name = "URI",
        env = "MQTT_BROKER_URI",
        default_value = "mqtt://127.0.0.1:1883"
    )]
    broker_uri: String,
    /// The MQTT client identifier.
    #[arg(
        long,
        value_name = "ID",
        env = "MQTT_CLIENT_ID",
        default_value = "anomaly-detector"
    )]
    client_id: String,
    /// The user name for authenticating to the MQTT broker.
    #[arg(long, value_name = "NAME", env = "MQTT_USERNAME")]
    username: Option<String>,
    /// The password for authenticating to the MQTT broker.
    #[arg(
        long,
        value_name = "PASSWORD",
        env = "MQTT_PASSWORD",
        hide_env_values = true
    )]
    password: Option<String>,
    /// The topic to read the AZ3166 telemetry from.
    #[arg(
        long,
        value_name = "TOPIC",
        env = "TELEMETRY_TOPIC",
        default_value = "ThreadXAZ3166/telemetry"
    )]
    telemetry_topic: String,
    /// The topic to publish the anomaly status to (retained, one message per telemetry sample).
    #[arg(
        long,
        value_name = "TOPIC",
        env = "ANOMALY_TOPIC",
        default_value = "vehicle/anomaly"
    )]
    anomaly_topic: String,
    /// A CSV file with telemetry of normal operation to train the model with.
    #[arg(
        long,
        value_name = "PATH",
        env = "TRAINING_DATA",
        default_value = "/app/training/az3166-baseline.csv"
    )]
    training_data: PathBuf,
    /// The number of standard deviations from the trained mean at which a value is out of range.
    #[arg(long, env = "LEVEL_THRESHOLD", default_value_t = 6.0)]
    level_threshold: f64,
    /// The number of standard deviations of the trained sample-to-sample change at which a change
    /// is sudden.
    #[arg(long, env = "CHANGE_THRESHOLD", default_value_t = 8.0)]
    change_threshold: f64,
}

#[derive(serde::Serialize)]
struct Status<'a> {
    timestamp: String,
    source: &'a str,
    mode: &'a str,
    anomaly: bool,
    findings: &'a [model::Finding],
}

async fn connect_and_subscribe(
    client: &mqtt::AsyncClient,
    connect_options: &mqtt::ConnectOptions,
    topic: &str,
) {
    let mut delay = Duration::from_secs(1);
    loop {
        let connected = if client.is_connected() {
            Ok(())
        } else {
            client.connect(connect_options.clone()).await.map(|_| ())
        };
        match connected {
            Ok(()) => match client.subscribe(topic, 1).await {
                Ok(_) => {
                    info!("Subscribed to telemetry topic {topic}");
                    return;
                }
                Err(e) => warn!("Cannot subscribe to {topic}: {e}"),
            },
            Err(e) => warn!("Cannot connect to MQTT broker: {e}"),
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let cli = Cli::parse();

    let training_data = std::fs::read_to_string(&cli.training_data).map_err(|e| {
        format!(
            "cannot read training data {}: {e}",
            cli.training_data.display()
        )
    })?;
    let model = model::Model::train_from_csv(&training_data)?;
    for mode in &model.modes {
        for signal in &mode.signals {
            info!(
                "Trained {} {}: normal range {:.2}..{:.2}, max change {:.2} per sample",
                mode.name,
                signal.name,
                signal.mean - cli.level_threshold * signal.std,
                signal.mean + cli.level_threshold * signal.std,
                cli.change_threshold * signal.delta_std
            );
        }
    }
    let mut detector = model::Detector::new(model, cli.level_threshold, cli.change_threshold);
    let source = cli
        .telemetry_topic
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();

    let mut client = mqtt::AsyncClient::new(
        mqtt::CreateOptionsBuilder::new()
            .server_uri(&cli.broker_uri)
            .client_id(&cli.client_id)
            .finalize(),
    )?;
    let messages = client.get_stream(64);
    let mut connect_options = mqtt::ConnectOptionsBuilder::new();
    connect_options
        .clean_session(true)
        .connect_timeout(Duration::from_secs(5));
    if let Some(username) = &cli.username {
        connect_options.user_name(username);
    }
    if let Some(password) = &cli.password {
        connect_options.password(password);
    }
    let connect_options = connect_options.finalize();

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut connected = false;
    let mut anomalous = false;
    let mut current_mode = String::new();
    loop {
        if !connected {
            tokio::select! {
                _ = sigterm.recv() => break,
                _ = sigint.recv() => break,
                _ = connect_and_subscribe(&client, &connect_options, &cli.telemetry_topic) => connected = true,
            }
        }
        let message = tokio::select! {
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
            message = messages.recv() => message,
        };
        let message = match message {
            Ok(Some(message)) => message,
            Ok(None) => {
                warn!("Lost connection to MQTT broker, reconnecting");
                connected = false;
                continue;
            }
            Err(_) => break,
        };
        let sample = model::parse_telemetry(&message.payload_str());
        if sample.is_empty() {
            debug!("Ignoring telemetry without known signals");
            continue;
        }
        let evaluation = detector.evaluate(&sample);
        if evaluation.mode != current_mode {
            info!("Telemetry matches trained mode {}", evaluation.mode);
            current_mode = evaluation.mode.to_string();
        }
        let findings = evaluation.findings;
        for finding in &findings {
            warn!(
                "Anomaly in {}: {:?}, value {} (normal {}..{}, score {})",
                finding.signal,
                finding.kind,
                finding.value,
                finding.normal_min,
                finding.normal_max,
                finding.score
            );
        }
        if findings.is_empty() {
            if anomalous {
                info!("Telemetry is back to normal");
            } else {
                debug!("Telemetry is normal");
            }
        }
        anomalous = !findings.is_empty();

        let status = Status {
            timestamp: humantime::format_rfc3339_millis(SystemTime::now()).to_string(),
            source: &source,
            mode: &current_mode,
            anomaly: anomalous,
            findings: &findings,
        };
        let payload = serde_json::to_vec(&status)?;
        let status_message = mqtt::MessageBuilder::new()
            .topic(&cli.anomaly_topic)
            .payload(payload)
            .qos(1)
            .retained(true)
            .finalize();
        if let Err(e) = client.publish(status_message).await {
            warn!("Failed to publish anomaly status: {e}");
        }
    }
    info!("Anomaly detector is shutting down");
    let _ = client.disconnect(None).await;
    Ok(())
}
