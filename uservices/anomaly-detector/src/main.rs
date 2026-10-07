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

//! Detects acceleration events and large temperature changes in the MXChip AZ3166 telemetry received
//! via MQTT, publishes the result and serves a dashboard.

use std::{
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use clap::Parser;
use log::{debug, info, warn};
use paho_mqtt as mqtt;
use tokio::signal::unix::{SignalKind, signal};

mod dashboard;
mod model;

// used for the first sample, the MXChip publishes every ~5 s
const DEFAULT_INTERVAL_SECS: f64 = 5.0;

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
    /// The accelerometer axis pointing in the vehicle's driving direction ([+-]x, [+-]y or [+-]z).
    #[arg(
        long,
        value_name = "AXIS",
        env = "FORWARD_AXIS",
        default_value = "+x",
        allow_hyphen_values = true,
        value_parser = model::ForwardAxis::from_str,
    )]
    forward_axis: model::ForwardAxis,
    /// The deviation of the acceleration from the resting baseline in mg (1000 mg = 1 g) from which
    /// on a sudden acceleration is reported.
    #[arg(long, value_name = "MG", env = "ACCEL_THRESHOLD_MG", default_value_t = 100.0)]
    accel_threshold_mg: f64,
    /// The longitudinal acceleration in mg from which on harsh acceleration or braking is reported.
    #[arg(long, value_name = "MG", env = "HARSH_EVENT_THRESHOLD_MG", default_value_t = 150.0)]
    harsh_event_threshold_mg: f64,
    /// The deviation of the temperature from its baseline in degrees Celsius from which on a
    /// temperature change is reported.
    #[arg(long, value_name = "CELSIUS", env = "TEMPERATURE_THRESHOLD_C", default_value_t = 3.0)]
    temperature_threshold_c: f64,
    /// The time constant in seconds with which the acceleration baseline follows a new resting
    /// position.
    #[arg(long, value_name = "SECONDS", env = "ACCEL_BASELINE_SECS", default_value_t = 60.0)]
    accel_baseline_secs: f64,
    /// The time constant in seconds with which the temperature baseline follows slow changes.
    #[arg(
        long,
        value_name = "SECONDS",
        env = "TEMPERATURE_BASELINE_SECS",
        default_value_t = 300.0
    )]
    temperature_baseline_secs: f64,
    /// The address to serve the read-only web dashboard on.
    #[arg(
        long,
        value_name = "ADDRESS",
        env = "HTTP_ADDRESS",
        default_value = "0.0.0.0:8090"
    )]
    http_address: SocketAddr,
}

#[derive(serde::Serialize)]
struct Status<'a> {
    timestamp: String,
    source: &'a str,
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

    let settings = model::Settings {
        forward_axis: cli.forward_axis,
        accel_threshold_mg: cli.accel_threshold_mg,
        harsh_threshold_mg: cli.harsh_event_threshold_mg,
        temperature_threshold_c: cli.temperature_threshold_c,
        accel_baseline_secs: cli.accel_baseline_secs,
        temperature_baseline_secs: cli.temperature_baseline_secs,
    };
    info!(
        "Monitoring acceleration (> {} mg from rest, harsh events > {} mg) and temperature (> {} °C from baseline)",
        settings.accel_threshold_mg, settings.harsh_threshold_mg, settings.temperature_threshold_c
    );
    let mut detector = model::Detector::new(settings);
    let source = cli
        .telemetry_topic
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();

    let dashboard_state = Arc::new(Mutex::new(dashboard::DashboardState::new(&source)));
    let http_address = cli.http_address;
    let server_state = dashboard_state.clone();
    tokio::spawn(async move {
        if let Err(e) = dashboard::serve(http_address, server_state).await {
            warn!("Dashboard stopped: {e}");
        }
    });

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
    let mut previous_sample: Option<Instant> = None;
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
        let now = Instant::now();
        let elapsed = previous_sample
            .map(|previous| now.duration_since(previous).as_secs_f64())
            .unwrap_or(DEFAULT_INTERVAL_SECS);
        previous_sample = Some(now);

        let evaluation = detector.evaluate(&sample, elapsed);
        for finding in &evaluation.findings {
            warn!(
                "Anomaly in {}: {:?}, value {} (normal {}..{})",
                finding.signal, finding.kind, finding.value, finding.normal_min, finding.normal_max
            );
        }
        if evaluation.findings.is_empty() {
            if anomalous {
                info!("Telemetry is back to normal");
            } else {
                debug!("Telemetry is normal");
            }
        }
        anomalous = !evaluation.findings.is_empty();

        let timestamp = humantime::format_rfc3339_millis(SystemTime::now()).to_string();
        dashboard_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .update(&timestamp, &evaluation.signals, &evaluation.findings);
        let status = Status {
            timestamp,
            source: &source,
            anomaly: anomalous,
            findings: &evaluation.findings,
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
