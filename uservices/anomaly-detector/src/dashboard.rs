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

//! Minimal read-only HTTP dashboard: current telemetry, normal ranges, anomaly status and recent
//! events. Serves three static files and one JSON endpoint; no external assets are needed.

use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use log::{debug, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::model::{Finding, SignalState};

// 10 minutes at the MXChip's 5 s interval
const HISTORY_LENGTH: usize = 120;
const EVENT_COUNT: usize = 50;
const MAX_REQUEST_SIZE: usize = 8192;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

#[derive(Debug, Default, serde::Serialize)]
struct SignalView {
    name: String,
    value: Option<f64>,
    /// None for signals that are shown but not monitored.
    normal_min: Option<f64>,
    normal_max: Option<f64>,
    anomalous: bool,
    history: VecDeque<f64>,
}

#[derive(Debug, serde::Serialize)]
struct Event {
    timestamp: String,
    #[serde(flatten)]
    finding: Finding,
}

#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct DashboardState {
    source: String,
    updated: Option<String>,
    anomaly: bool,
    samples: u64,
    signals: Vec<SignalView>,
    findings: Vec<Finding>,
    events: VecDeque<Event>,
}

pub(crate) type SharedState = Arc<Mutex<DashboardState>>;

impl DashboardState {
    pub(crate) fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            ..Default::default()
        }
    }

    pub(crate) fn update(&mut self, timestamp: &str, signals: &[SignalState], findings: &[Finding]) {
        self.updated = Some(timestamp.to_string());
        self.anomaly = !findings.is_empty();
        self.samples += 1;
        self.findings = findings.to_vec();
        for signal in signals {
            let index = match self.signals.iter().position(|s| s.name == signal.name) {
                Some(index) => index,
                None => {
                    self.signals.push(SignalView {
                        name: signal.name.clone(),
                        ..Default::default()
                    });
                    self.signals.len() - 1
                }
            };
            let view = &mut self.signals[index];
            view.value = Some(signal.value);
            view.normal_min = signal.normal.map(|(min, _)| min);
            view.normal_max = signal.normal.map(|(_, max)| max);
            view.anomalous = findings.iter().any(|f| f.signal == signal.name);
            if view.history.len() == HISTORY_LENGTH {
                view.history.pop_front();
            }
            view.history.push_back(signal.value);
        }
        // signals that appear later (e.g. the model score after its warm-up) keep the detector's order
        self.signals.sort_by_key(|view| {
            signals
                .iter()
                .position(|s| s.name == view.name)
                .unwrap_or(usize::MAX)
        });
        for finding in findings {
            if self.events.len() == EVENT_COUNT {
                self.events.pop_back();
            }
            self.events.push_front(Event {
                timestamp: timestamp.to_string(),
                finding: finding.clone(),
            });
        }
    }
}

pub(crate) async fn serve(address: SocketAddr, state: SharedState) -> std::io::Result<()> {
    let listener = TcpListener::bind(address).await?;
    info!("Dashboard available at http://{address}/");
    loop {
        let (stream, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(5), handle(stream, state)).await {
                Ok(Err(e)) => debug!("Dashboard request from {peer} failed: {e}"),
                Err(_) => debug!("Dashboard request from {peer} timed out"),
                Ok(Ok(())) => {}
            }
        });
    }
}

async fn handle(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let mut buffer = vec![0u8; MAX_REQUEST_SIZE];
    let mut length = 0;
    while !buffer[..length].windows(4).any(|w| w == b"\r\n\r\n") && length < buffer.len() {
        let read = stream.read(&mut buffer[length..]).await?;
        if read == 0 {
            return Ok(());
        }
        length += read;
    }
    let request = String::from_utf8_lossy(&buffer[..length]);
    let mut request_line = request.lines().next().unwrap_or_default().split_whitespace();
    let method = request_line.next().unwrap_or_default();
    let path = request_line.next().unwrap_or_default();

    let (status, content_type, body): (&str, &str, Vec<u8>) = match (method, path) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", INDEX_HTML.into()),
        ("GET", "/app.js") => ("200 OK", "text/javascript; charset=utf-8", APP_JS.into()),
        ("GET", "/style.css") => ("200 OK", "text/css; charset=utf-8", STYLE_CSS.into()),
        ("GET", "/api/status") => {
            let state = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                "200 OK",
                "application/json",
                serde_json::to_vec(&*state).unwrap_or_default(),
            )
        }
        ("GET", _) => ("404 Not Found", "text/plain", b"not found".to_vec()),
        _ => ("405 Method Not Allowed", "text/plain", b"method not allowed".to_vec()),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nContent-Security-Policy: default-src 'self'\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AnomalyKind;

    fn humidity(value: f64, normal: Option<(f64, f64)>) -> SignalState {
        SignalState {
            name: "humidity".to_string(),
            value,
            normal,
        }
    }

    #[test]
    fn keeps_bounded_history_and_events() {
        let mut state = DashboardState::new("ThreadXAZ3166");
        let finding = Finding {
            detector: "rules",
            signal: "humidity".to_string(),
            kind: AnomalyKind::TemperatureChange,
            cause: "test".to_string(),
            value: 20.0,
            score: 9.0,
            normal_min: 0.0,
            normal_max: 10.0,
        };
        for i in 0..200 {
            state.update("t", &[humidity(i as f64, Some((0.0, 10.0)))], &[finding.clone()]);
        }
        assert_eq!(state.samples, 200);
        assert_eq!(state.signals.len(), 1);
        assert_eq!(state.signals[0].history.len(), HISTORY_LENGTH);
        assert_eq!(state.signals[0].value, Some(199.0));
        assert!(state.signals[0].anomalous);
        assert_eq!(state.events.len(), EVENT_COUNT);
    }

    #[test]
    fn normal_sample_clears_anomaly() {
        let mut state = DashboardState::new("ThreadXAZ3166");
        state.update("t", &[humidity(5.0, None)], &[]);
        assert!(!state.anomaly);
        assert!(!state.signals[0].anomalous);
        assert_eq!(state.signals[0].normal_min, None);
        assert!(state.events.is_empty());
    }
}
