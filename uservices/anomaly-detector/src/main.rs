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

//! Detects anomalies in the MXChip AZ3166 telemetry received via MQTT with an Isolation Forest
//! trained on recorded normal data and with explainable rules (acceleration events, large
//! temperature changes), publishes the result and serves a dashboard.

use std::{
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use clap::Parser;
use log::{debug, info, warn};
use paho_mqtt as mqtt;
use tokio::signal::unix::{SignalKind, signal};

mod dashboard;
mod forest;
mod model;
mod uprotocol;

// used for the first sample, the MXChip publishes every ~5 s
const DEFAULT_INTERVAL_SECS: f64 = 5.0;
// longer gaps (e.g. broker outage) restart the Isolation Forest's sliding window
const MAX_WINDOW_GAP_SECS: f64 = 60.0;

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
    /// A uProtocol topic to also receive the AZ3166 telemetry from via MQTT 5 on the same broker
    /// (e.g. up://az3166/AB/1/8001), with a payload as on TELEMETRY_TOPIC or as JSON.
    #[arg(
        long,
        value_name = "URI",
        env = "UPROTOCOL_TELEMETRY_TOPIC",
        value_parser = up_rust::UUri::from_str,
    )]
    uprotocol_telemetry_topic: Option<up_rust::UUri>,
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
    /// A CSV file with consecutive telemetry of normal operation to train the Isolation Forest with
    /// (see training/recording_to_csv.py).
    #[arg(
        long,
        value_name = "PATH",
        env = "TRAINING_DATA",
        default_value = "/app/training/az3166-normal.csv"
    )]
    training_data: PathBuf,
    /// The minimum Isolation Forest score (0..1) for an anomaly; the threshold is raised above the
    /// highest score of the training data if needed.
    #[arg(long, value_name = "SCORE", env = "MODEL_SCORE_THRESHOLD", default_value_t = 0.6)]
    model_score_threshold: f64,
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
    model_score: Option<f64>,
    model_threshold: f64,
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

    let training_data = std::fs::read_to_string(&cli.training_data).map_err(|e| {
        format!(
            "cannot read training data {}: {e}",
            cli.training_data.display()
        )
    })?;
    let forest = forest::IsolationForestModel::train(
        &forest::read_training_csv(&training_data)?,
        cli.model_score_threshold,
    )?;
    info!(
        "Trained Isolation Forest on {} feature vectors from {} (window {} samples): highest training score {:.3}, anomaly threshold {:.3}",
        forest.training_vectors,
        cli.training_data.display(),
        forest::WINDOW,
        forest.max_training_score,
        forest.threshold
    );
    let mut features = forest::FeatureExtractor::default();
    let threshold = (forest.threshold * 1000.0).round() / 1000.0;
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

    // the unused sender keeps the receiver pending if no uProtocol topic is configured
    let (_no_uprotocol, mut uprotocol_payloads) = tokio::sync::mpsc::channel::<String>(1);
    if let Some(topic) = cli.uprotocol_telemetry_topic.clone() {
        uprotocol_payloads = uprotocol::subscribe(
            up_transport_mqtt5::MqttClientOptions {
                client_id: Some(format!("{}-uprotocol", cli.client_id)),
                broker_uri: cli.broker_uri.clone(),
                username: cli.username.clone(),
                password: cli.password.clone(),
                ..Default::default()
            },
            topic,
        );
    }

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
        let payload = tokio::select! {
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
            message = messages.recv() => match message {
                Ok(Some(message)) => message.payload_str().into_owned(),
                Ok(None) => {
                    warn!("Lost connection to MQTT broker, reconnecting");
                    connected = false;
                    continue;
                }
                Err(_) => break,
            },
            Some(payload) = uprotocol_payloads.recv() => payload,
        };
        let sample = model::parse_telemetry(&payload);
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
        let mut findings = evaluation.findings;
        let mut signals = evaluation.signals;

        if elapsed > MAX_WINDOW_GAP_SECS {
            features.reset();
        }
        let mut model_score = None;
        if let (Some(&x), Some(&y), Some(&z), Some(&temperature)) = (
            sample.get("accel_x"),
            sample.get("accel_y"),
            sample.get("accel_z"),
            sample.get("temperature"),
        ) {
            if let Some(vector) = features.push([x, y, z], temperature) {
                let score = (forest.score(&vector) * 1000.0).round() / 1000.0;
                model_score = Some(score);
                signals.insert(
                    0,
                    model::SignalState {
                        name: "isolation_forest_score".to_string(),
                        value: score,
                        normal: Some((0.0, threshold)),
                    },
                );
                if score > threshold {
                    let cause = forest.cause(&vector);
                    findings.insert(
                        0,
                        model::Finding {
                            detector: "isolation_forest",
                            signal: "isolation_forest_score".to_string(),
                            kind: model::AnomalyKind::IsolationForest,
                            cause: format!(
                                "unusual {}: {:.1} {} (at most {:.1} {} in normal data), score {:.3} > {:.3}",
                                cause.feature,
                                cause.value,
                                cause.unit,
                                cause.training_max,
                                cause.unit,
                                score,
                                threshold
                            ),
                            value: score,
                            score: (score / threshold * 100.0).round() / 100.0,
                            normal_min: 0.0,
                            normal_max: threshold,
                        },
                    );
                }
            }
        }

        for finding in &findings {
            warn!("ANOMALY detected by {}: {}", finding.detector, finding.cause);
        }
        if findings.is_empty() {
            if anomalous {
                info!("Telemetry is back to normal");
            } else {
                debug!("Telemetry is normal (model score {model_score:?})");
            }
        }
        anomalous = !findings.is_empty();

        let timestamp = humantime::format_rfc3339_millis(SystemTime::now()).to_string();
        dashboard_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .update(&timestamp, &signals, &findings);
        let status = Status {
            timestamp,
            source: &source,
            anomaly: anomalous,
            model_score,
            model_threshold: threshold,
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
