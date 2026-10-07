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

//! Adaptive anomaly detection for the MXChip AZ3166: only acceleration events and large temperature
//! changes are reported. Each monitored value is compared with a slowly adapting baseline
//! (exponential moving average), so a new resting position or a slow warm-up becomes the new normal.

use std::{collections::HashMap, str::FromStr};

const ACCEL_SIGNALS: [&str; 3] = ["accel_x", "accel_y", "accel_z"];
// shown on the dashboard, but never reported as anomalies
const OTHER_SIGNALS: [&str; 5] = ["pressure", "humidity", "mag_x", "mag_y", "mag_z"];

/// Parses the AZ3166 telemetry text, e.g. `Pressure: 965.65` or `Acceleration: 4.51, -26.53, 1023.89`
/// (one line per sensor). Unknown or malformed lines are skipped.
pub(crate) fn parse_telemetry(payload: &str) -> HashMap<&'static str, f64> {
    let mut sample = HashMap::new();
    for line in payload.lines() {
        let Some((key, values)) = line.split_once(':') else {
            continue;
        };
        let names: &[&'static str] = match key.trim() {
            "Pressure" => &["pressure"],
            "Temperature" => &["temperature"],
            "Humidity" => &["humidity"],
            "Acceleration" => &ACCEL_SIGNALS,
            "Magnetic" => &["mag_x", "mag_y", "mag_z"],
            _ => continue,
        };
        let parsed: Vec<f64> = values
            .split(',')
            .filter_map(|value| value.trim().parse().ok())
            .collect();
        if parsed.len() == names.len() {
            sample.extend(names.iter().copied().zip(parsed));
        }
    }
    sample
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnomalyKind {
    /// The acceleration differs from the resting baseline (gravity) by more than the threshold.
    SuddenAcceleration,
    /// Acceleration along the forward axis beyond the harsh event threshold.
    HarshAcceleration,
    /// Deceleration along the forward axis beyond the harsh event threshold.
    HarshBraking,
    /// The temperature differs from its baseline by more than the threshold.
    TemperatureChange,
}

/// The accelerometer axis that points in the vehicle's driving direction, e.g. `+x` or `-y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ForwardAxis {
    index: usize,
    sign: f64,
}

impl FromStr for ForwardAxis {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (sign, axis) = match s.trim() {
            s if s.starts_with('-') => (-1.0, &s[1..]),
            s if s.starts_with('+') => (1.0, &s[1..]),
            s => (1.0, s),
        };
        let index = match axis {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            _ => return Err(format!("invalid axis {s}, expected [+-]x, [+-]y or [+-]z")),
        };
        Ok(Self { index, sign })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Settings {
    pub forward_axis: ForwardAxis,
    /// Deviation of the acceleration vector from the resting baseline, in mg (1000 mg = 1 g).
    pub accel_threshold_mg: f64,
    /// Longitudinal acceleration along the forward axis for harsh acceleration/braking, in mg.
    pub harsh_threshold_mg: f64,
    /// Deviation of the temperature from its baseline, in degrees Celsius.
    pub temperature_threshold_c: f64,
    /// Time constant of the acceleration baseline in seconds.
    pub accel_baseline_secs: f64,
    /// Time constant of the temperature baseline in seconds.
    pub temperature_baseline_secs: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct Finding {
    pub signal: String,
    pub kind: AnomalyKind,
    pub value: f64,
    /// Value relative to the threshold (> 1 is an anomaly).
    pub score: f64,
    pub normal_min: f64,
    pub normal_max: f64,
}

/// A signal's current value and, if it is monitored, its current normal range.
#[derive(Debug, Clone)]
pub(crate) struct SignalState {
    pub name: String,
    pub value: f64,
    pub normal: Option<(f64, f64)>,
}

pub(crate) struct Evaluation {
    pub findings: Vec<Finding>,
    pub signals: Vec<SignalState>,
}

pub(crate) struct Detector {
    settings: Settings,
    gravity: Option<[f64; 3]>,
    temperature: Option<f64>,
}

/// Weight of a new sample in an exponential moving average with the given time constant.
fn smoothing(elapsed_secs: f64, time_constant_secs: f64) -> f64 {
    1.0 - (-elapsed_secs / time_constant_secs).exp()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn signal(name: &str, value: f64, normal: Option<(f64, f64)>) -> SignalState {
    SignalState {
        name: name.to_string(),
        value: round2(value),
        normal: normal.map(|(min, max)| (round2(min), round2(max))),
    }
}

impl Detector {
    pub(crate) fn new(settings: Settings) -> Self {
        Self {
            settings,
            gravity: None,
            temperature: None,
        }
    }

    /// Evaluates a sample received `elapsed_secs` after the previous one.
    pub(crate) fn evaluate(&mut self, sample: &HashMap<&str, f64>, elapsed_secs: f64) -> Evaluation {
        let mut findings = Vec::new();
        let mut signals = Vec::new();
        self.evaluate_acceleration(sample, elapsed_secs, &mut findings, &mut signals);
        self.evaluate_temperature(sample, elapsed_secs, &mut findings, &mut signals);
        for name in OTHER_SIGNALS {
            if let Some(&value) = sample.get(name) {
                signals.push(signal(name, value, None));
            }
        }
        Evaluation { findings, signals }
    }

    fn evaluate_acceleration(
        &mut self,
        sample: &HashMap<&str, f64>,
        elapsed_secs: f64,
        findings: &mut Vec<Finding>,
        signals: &mut Vec<SignalState>,
    ) {
        let Some(accel) = ACCEL_SIGNALS
            .iter()
            .map(|name| sample.get(name).copied())
            .collect::<Option<Vec<f64>>>()
        else {
            return;
        };
        let settings = self.settings;
        // after a long gap (e.g. broker outage) start over instead of comparing with stale data
        let gravity = match self.gravity {
            Some(gravity) if elapsed_secs < 3.0 * settings.accel_baseline_secs => gravity,
            _ => [accel[0], accel[1], accel[2]],
        };
        let dynamic: Vec<f64> = accel.iter().zip(gravity).map(|(a, g)| a - g).collect();
        let magnitude = dynamic.iter().map(|d| d * d).sum::<f64>().sqrt();
        let longitudinal = settings.forward_axis.sign * dynamic[settings.forward_axis.index];

        if magnitude > settings.accel_threshold_mg {
            findings.push(Finding {
                signal: "acceleration".to_string(),
                kind: AnomalyKind::SuddenAcceleration,
                value: round2(magnitude),
                score: round2(magnitude / settings.accel_threshold_mg),
                normal_min: 0.0,
                normal_max: settings.accel_threshold_mg,
            });
        }
        let harsh = if longitudinal > settings.harsh_threshold_mg {
            Some(AnomalyKind::HarshAcceleration)
        } else if longitudinal < -settings.harsh_threshold_mg {
            Some(AnomalyKind::HarshBraking)
        } else {
            None
        };
        if let Some(kind) = harsh {
            findings.push(Finding {
                signal: "longitudinal_accel".to_string(),
                kind,
                value: round2(longitudinal),
                score: round2(longitudinal.abs() / settings.harsh_threshold_mg),
                normal_min: -settings.harsh_threshold_mg,
                normal_max: settings.harsh_threshold_mg,
            });
        }

        signals.push(signal(
            "acceleration",
            magnitude,
            Some((0.0, settings.accel_threshold_mg)),
        ));
        signals.push(signal(
            "longitudinal_accel",
            longitudinal,
            Some((-settings.harsh_threshold_mg, settings.harsh_threshold_mg)),
        ));
        for (index, name) in ACCEL_SIGNALS.iter().enumerate() {
            signals.push(signal(
                name,
                accel[index],
                Some((
                    gravity[index] - settings.accel_threshold_mg,
                    gravity[index] + settings.accel_threshold_mg,
                )),
            ));
        }

        let weight = smoothing(elapsed_secs, settings.accel_baseline_secs);
        self.gravity = Some([
            gravity[0] + weight * dynamic[0],
            gravity[1] + weight * dynamic[1],
            gravity[2] + weight * dynamic[2],
        ]);
    }

    fn evaluate_temperature(
        &mut self,
        sample: &HashMap<&str, f64>,
        elapsed_secs: f64,
        findings: &mut Vec<Finding>,
        signals: &mut Vec<SignalState>,
    ) {
        let Some(&value) = sample.get("temperature") else {
            return;
        };
        let settings = self.settings;
        let baseline = match self.temperature {
            Some(baseline) if elapsed_secs < 3.0 * settings.temperature_baseline_secs => baseline,
            _ => value,
        };
        let (min, max) = (
            baseline - settings.temperature_threshold_c,
            baseline + settings.temperature_threshold_c,
        );
        if value < min || value > max {
            findings.push(Finding {
                signal: "temperature".to_string(),
                kind: AnomalyKind::TemperatureChange,
                value: round2(value),
                score: round2((value - baseline).abs() / settings.temperature_threshold_c),
                normal_min: round2(min),
                normal_max: round2(max),
            });
        }
        signals.push(signal("temperature", value, Some((min, max))));
        self.temperature = Some(
            baseline + smoothing(elapsed_secs, settings.temperature_baseline_secs) * (value - baseline),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // the MXChip publishes every ~5 s
    const INTERVAL: f64 = 5.0;
    // real telemetry of the device lying still
    const RESTING: &str = "Pressure: 965.06\nTemperature: 28.98\nHumidity: 50.02\n\
        Acceleration: -9.09, -14.34, 1023.15\nMagnetic: 108.00, 216.00, -516.00\n";

    fn settings() -> Settings {
        Settings {
            forward_axis: "+x".parse().unwrap(),
            accel_threshold_mg: 100.0,
            harsh_threshold_mg: 150.0,
            temperature_threshold_c: 3.0,
            accel_baseline_secs: 60.0,
            temperature_baseline_secs: 300.0,
        }
    }

    fn with(replace: &str, by: &str) -> HashMap<&'static str, f64> {
        parse_telemetry(&RESTING.replace(replace, by))
    }

    fn kinds(evaluation: &Evaluation) -> Vec<AnomalyKind> {
        evaluation.findings.iter().map(|f| f.kind).collect()
    }

    fn rested_detector() -> Detector {
        let mut detector = Detector::new(settings());
        for _ in 0..12 {
            detector.evaluate(&parse_telemetry(RESTING), INTERVAL);
        }
        detector
    }

    #[test]
    fn parses_all_signals() {
        let sample = parse_telemetry(RESTING);
        assert_eq!(sample.len(), 9);
        assert_eq!(sample["pressure"], 965.06);
        assert_eq!(sample["accel_y"], -14.34);
        assert_eq!(sample["mag_z"], -516.0);
    }

    #[test]
    fn skips_malformed_lines() {
        let sample = parse_telemetry("Pressure: abc\nAcceleration: 1, 2\nFoo: 1\nHumidity: 50.0");
        assert_eq!(sample.len(), 1);
        assert_eq!(sample["humidity"], 50.0);
    }

    #[test]
    fn parses_forward_axis() {
        assert_eq!("+x".parse(), Ok(ForwardAxis { index: 0, sign: 1.0 }));
        assert_eq!("-y".parse(), Ok(ForwardAxis { index: 1, sign: -1.0 }));
        assert_eq!("z".parse(), Ok(ForwardAxis { index: 2, sign: 1.0 }));
        assert!("w".parse::<ForwardAxis>().is_err());
    }

    #[test]
    fn first_sample_is_normal() {
        let mut detector = Detector::new(settings());
        let evaluation = detector.evaluate(&parse_telemetry(RESTING), INTERVAL);
        assert!(evaluation.findings.is_empty());
        assert_eq!(evaluation.signals.len(), 2 + 9);
    }

    #[test]
    fn sensor_jitter_is_normal() {
        let mut detector = rested_detector();
        let jitter = with("-9.09, -14.34, 1023.15", "-9.15, -14.21, 1021.75");
        assert!(detector.evaluate(&jitter, INTERVAL).findings.is_empty());
    }

    #[test]
    fn small_tilt_is_normal() {
        let mut detector = rested_detector();
        // about 4 degrees, as during the 5 minute recording
        let tilted = with("-9.09, -14.34", "-9.09, -79.00");
        assert!(detector.evaluate(&tilted, INTERVAL).findings.is_empty());
    }

    #[test]
    fn new_resting_position_becomes_normal() {
        let mut detector = rested_detector();
        let turned = with("-9.09, -14.34, 1023.15", "-9.09, -400.00, 940.00");
        assert_eq!(
            kinds(&detector.evaluate(&turned, INTERVAL)),
            [AnomalyKind::SuddenAcceleration]
        );
        let normal_after = (0..60)
            .position(|_| detector.evaluate(&turned, INTERVAL).findings.is_empty())
            .expect("the new position becomes normal");
        assert!(normal_after * 5 < 120, "took {} s", normal_after * 5);
    }

    #[test]
    fn harsh_braking_is_detected() {
        let mut detector = rested_detector();
        let braking = with("-9.09, -14.34", "-259.09, -14.34");
        let evaluation = detector.evaluate(&braking, INTERVAL);
        assert_eq!(
            kinds(&evaluation),
            [AnomalyKind::SuddenAcceleration, AnomalyKind::HarshBraking]
        );
        assert!(evaluation.findings[1].value < -150.0);
    }

    #[test]
    fn harsh_acceleration_along_negative_axis_is_detected() {
        let mut detector = Detector::new(Settings {
            forward_axis: "-y".parse().unwrap(),
            ..settings()
        });
        detector.evaluate(&parse_telemetry(RESTING), INTERVAL);
        let accelerating = with("-14.34", "-214.34");
        assert!(kinds(&detector.evaluate(&accelerating, INTERVAL)).contains(&AnomalyKind::HarshAcceleration));
    }

    #[test]
    fn large_temperature_change_is_detected() {
        let mut detector = rested_detector();
        let hot = with("Temperature: 28.98", "Temperature: 33.50");
        let evaluation = detector.evaluate(&hot, INTERVAL);
        assert_eq!(kinds(&evaluation), [AnomalyKind::TemperatureChange]);
        assert_eq!(evaluation.findings[0].value, 33.5);
    }

    #[test]
    fn slow_warm_up_is_normal() {
        let mut detector = rested_detector();
        for step in 1..=120 {
            // 2 degrees over 10 minutes
            let warmer = format!("Temperature: {:.2}", 28.98 + step as f64 * 2.0 / 120.0);
            let sample = with("Temperature: 28.98", &warmer);
            assert!(detector.evaluate(&sample, INTERVAL).findings.is_empty());
        }
    }

    #[test]
    fn magnetometer_humidity_and_pressure_are_not_monitored() {
        let mut detector = rested_detector();
        let disturbed = parse_telemetry(
            &RESTING
                .replace("108.00, 216.00, -516.00", "-400.00, 600.00, 100.00")
                .replace("Humidity: 50.02", "Humidity: 80.00")
                .replace("Pressure: 965.06", "Pressure: 900.00"),
        );
        let evaluation = detector.evaluate(&disturbed, INTERVAL);
        assert!(evaluation.findings.is_empty());
        let mag = evaluation.signals.iter().find(|s| s.name == "mag_x").unwrap();
        assert!(mag.normal.is_none());
    }

    #[test]
    fn long_gap_restarts_the_baseline() {
        let mut detector = rested_detector();
        let turned = with("-9.09, -14.34, 1023.15", "-9.09, -400.00, 940.00");
        assert!(detector.evaluate(&turned, 3600.0).findings.is_empty());
    }
}
