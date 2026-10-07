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

//! Statistical baseline model: for each operating mode (e.g. a known position of the device) and each
//! signal, the mean and standard deviation of its level and of the change between consecutive samples,
//! learned from training data.

use std::{collections::HashMap, str::FromStr};

const MODE_COLUMN: &str = "mode";
const DEFAULT_MODE: &str = "default";
const ACCEL_SIGNALS: [&str; 3] = ["accel_x", "accel_y", "accel_z"];

/// Smallest standard deviation used per signal: the sensor's resolution/tolerance, so that a short
/// recording of a device lying still does not turn every jitter into an anomaly.
fn min_std(signal: &str) -> f64 {
    match signal {
        "pressure" => 0.25,
        "temperature" => 0.25,
        "humidity" => 1.0,
        s if s.starts_with("accel_") || s.starts_with("mag_") => 5.0,
        _ => 1e-3,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SignalModel {
    pub name: String,
    pub mean: f64,
    pub std: f64,
    pub delta_std: f64,
}

#[derive(Debug)]
pub(crate) struct Mode {
    pub name: String,
    pub signals: Vec<SignalModel>,
}

impl Mode {
    /// Sum of the squared distances (in standard deviations) of the sample from this mode's means.
    fn distance(&self, sample: &HashMap<&str, f64>) -> f64 {
        self.signals
            .iter()
            .filter_map(|signal| {
                sample
                    .get(signal.name.as_str())
                    .map(|value| ((value - signal.mean) / signal.std).powi(2))
            })
            .sum()
    }
}

#[derive(Debug)]
pub(crate) struct Model {
    pub modes: Vec<Mode>,
}

impl Model {
    /// Trains the model from CSV data with a header row naming the signals and one sample per row.
    /// An optional first column named `mode` assigns each row to an operating mode.
    pub(crate) fn train_from_csv(csv: &str) -> Result<Self, String> {
        let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
        let mut header: Vec<&str> = lines
            .next()
            .ok_or("training data is empty")?
            .split(',')
            .map(str::trim)
            .collect();
        let has_mode_column = header.first() == Some(&MODE_COLUMN);
        if has_mode_column {
            header.remove(0);
        }
        // rows per mode, in order of first appearance
        let mut modes: Vec<(String, Vec<Vec<f64>>)> = Vec::new();
        for (index, line) in lines.enumerate() {
            let mut values: Vec<&str> = line.split(',').map(str::trim).collect();
            let mode = if has_mode_column && !values.is_empty() {
                values.remove(0)
            } else {
                DEFAULT_MODE
            };
            if values.len() != header.len() {
                return Err(format!(
                    "row {}: expected {} values but found {}",
                    index + 2,
                    header.len(),
                    values.len()
                ));
            }
            let row = values
                .iter()
                .map(|value| value.parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("row {}: {e}", index + 2))?;
            match modes.iter_mut().find(|(name, _)| name == mode) {
                Some((_, rows)) => rows.push(row),
                None => modes.push((mode.to_string(), vec![row])),
            }
        }
        if modes.is_empty() {
            return Err("training data has no samples".to_string());
        }
        let modes = modes
            .into_iter()
            .map(|(name, rows)| train_mode(name, &header, &rows))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { modes })
    }
}

fn train_mode(name: String, header: &[&str], rows: &[Vec<f64>]) -> Result<Mode, String> {
    if rows.len() < 2 {
        return Err(format!("mode {name}: at least 2 samples are needed"));
    }
    let signals = header
        .iter()
        .enumerate()
        .map(|(column, signal)| {
            let values: Vec<f64> = rows.iter().map(|row| row[column]).collect();
            let deltas: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
            let (mean, std) = mean_std(&values);
            let (_, delta_std) = mean_std(&deltas);
            SignalModel {
                name: signal.to_string(),
                mean,
                std: std.max(min_std(signal)),
                delta_std: delta_std.max(min_std(signal)),
            }
        })
        .collect();
    Ok(Mode { name, signals })
}

fn mean_std(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

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
            "Acceleration" => &["accel_x", "accel_y", "accel_z"],
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
    /// The value is far away from the trained mean.
    OutOfRange,
    /// The value changed much more since the previous sample than in the training data.
    SuddenChange,
    /// Acceleration along the forward axis beyond the event threshold.
    HarshAcceleration,
    /// Deceleration along the forward axis beyond the event threshold.
    HarshBraking,
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

/// Detects harsh acceleration and braking from the change along the forward axis compared to the
/// trained gravity baseline at rest.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MotionEvents {
    pub forward_axis: ForwardAxis,
    /// Minimum longitudinal acceleration in mg that counts as an event.
    pub threshold_mg: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct Finding {
    pub signal: String,
    pub kind: AnomalyKind,
    pub value: f64,
    /// Distance in standard deviations.
    pub score: f64,
    pub normal_min: f64,
    pub normal_max: f64,
}

pub(crate) struct Evaluation<'a> {
    /// The trained mode that matches the sample best.
    pub mode: &'a str,
    pub findings: Vec<Finding>,
    /// The out-of-range limits of the matched mode.
    pub ranges: Vec<NormalRange>,
}

#[derive(Debug, Clone)]
pub(crate) struct NormalRange {
    pub signal: String,
    pub min: f64,
    pub max: f64,
}

pub(crate) struct Detector {
    model: Model,
    level_threshold: f64,
    change_threshold: f64,
    motion_events: Option<MotionEvents>,
    previous: HashMap<String, f64>,
}

impl Detector {
    pub(crate) fn new(
        model: Model,
        level_threshold: f64,
        change_threshold: f64,
        motion_events: Option<MotionEvents>,
    ) -> Self {
        Self {
            model,
            level_threshold,
            change_threshold,
            motion_events,
            previous: HashMap::new(),
        }
    }

    pub(crate) fn evaluate(&mut self, sample: &HashMap<&str, f64>) -> Evaluation<'_> {
        let mode = self
            .model
            .modes
            .iter()
            .min_by(|a, b| a.distance(sample).total_cmp(&b.distance(sample)))
            .expect("a trained model has at least one mode");
        let mut findings = Vec::new();
        for signal in &mode.signals {
            let Some(&value) = sample.get(signal.name.as_str()) else {
                continue;
            };
            let level_score = (value - signal.mean).abs() / signal.std;
            if level_score > self.level_threshold {
                findings.push(Finding {
                    signal: signal.name.clone(),
                    kind: AnomalyKind::OutOfRange,
                    value,
                    score: round2(level_score),
                    normal_min: round2(signal.mean - self.level_threshold * signal.std),
                    normal_max: round2(signal.mean + self.level_threshold * signal.std),
                });
            }
            if let Some(previous) = self.previous.insert(signal.name.clone(), value) {
                let change_score = (value - previous).abs() / signal.delta_std;
                if change_score > self.change_threshold {
                    findings.push(Finding {
                        signal: signal.name.clone(),
                        kind: AnomalyKind::SuddenChange,
                        value,
                        score: round2(change_score),
                        normal_min: round2(previous - self.change_threshold * signal.delta_std),
                        normal_max: round2(previous + self.change_threshold * signal.delta_std),
                    });
                }
            }
        }
        if let Some(motion) = self.motion_events {
            let name = ACCEL_SIGNALS[motion.forward_axis.index];
            if let (Some(axis), Some(&value)) =
                (mode.signals.iter().find(|s| s.name == name), sample.get(name))
            {
                let longitudinal = motion.forward_axis.sign * (value - axis.mean);
                // never closer to the noise floor than the out-of-range threshold
                let threshold = motion.threshold_mg.max(self.level_threshold * axis.std);
                let kind = if longitudinal > threshold {
                    Some(AnomalyKind::HarshAcceleration)
                } else if longitudinal < -threshold {
                    Some(AnomalyKind::HarshBraking)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    findings.push(Finding {
                        signal: "longitudinal_accel".to_string(),
                        kind,
                        value: round2(longitudinal),
                        score: round2(longitudinal.abs() / threshold),
                        normal_min: round2(-threshold),
                        normal_max: round2(threshold),
                    });
                }
            }
        }
        Evaluation {
            mode: &mode.name,
            findings,
            ranges: mode
                .signals
                .iter()
                .map(|signal| NormalRange {
                    signal: signal.name.clone(),
                    min: round2(signal.mean - self.level_threshold * signal.std),
                    max: round2(signal.mean + self.level_threshold * signal.std),
                })
                .collect(),
        }
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = include_str!("../training/az3166-baseline.csv");
    const POSITION_1: &str = "Pressure: 965.65\nTemperature: 31.16\nHumidity: 47.53\n\
        Acceleration: 4.51, -26.53, 1023.89\nMagnetic: 616.50, 301.50, 586.50\n";
    // real telemetry after the device was turned on the desk
    const POSITION_2: [&str; 3] = [
        "Pressure: 965.52\nTemperature: 31.51\nHumidity: 48.32\n\
        Acceleration: 9.76, -18.36, 1021.93\nMagnetic: 421.50, 271.50, -202.50\n",
        "Pressure: 965.52\nTemperature: 31.50\nHumidity: 48.64\n\
        Acceleration: 9.70, -18.42, 1020.65\nMagnetic: 412.50, 271.50, -202.50\n",
        "Pressure: 965.54\nTemperature: 31.48\nHumidity: 48.59\n\
        Acceleration: 9.58, -18.48, 1019.68\nMagnetic: 412.50, 270.00, -210.00\n",
    ];

    fn detector() -> Detector {
        Detector::new(
            Model::train_from_csv(BASELINE).unwrap(),
            6.0,
            8.0,
            Some(MotionEvents {
                forward_axis: "+x".parse().unwrap(),
                threshold_mg: 150.0,
            }),
        )
    }

    fn kinds(findings: &[Finding]) -> Vec<AnomalyKind> {
        findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn parses_forward_axis() {
        assert_eq!("+x".parse(), Ok(ForwardAxis { index: 0, sign: 1.0 }));
        assert_eq!("-y".parse(), Ok(ForwardAxis { index: 1, sign: -1.0 }));
        assert_eq!("z".parse(), Ok(ForwardAxis { index: 2, sign: 1.0 }));
        assert!("w".parse::<ForwardAxis>().is_err());
    }

    #[test]
    fn braking_along_forward_axis_is_detected() {
        let mut detector = detector();
        let braking = POSITION_1.replace("4.51, -26.53", "-295.49, -26.53");
        let findings = detector.evaluate(&parse_telemetry(&braking)).findings;
        let event = findings.iter().find(|f| f.signal == "longitudinal_accel").unwrap();
        assert_eq!(event.kind, AnomalyKind::HarshBraking);
        assert!(event.value < -150.0);
    }

    #[test]
    fn acceleration_along_negative_axis_is_detected() {
        let mut detector = Detector::new(
            Model::train_from_csv(BASELINE).unwrap(),
            6.0,
            8.0,
            Some(MotionEvents {
                forward_axis: "-y".parse().unwrap(),
                threshold_mg: 150.0,
            }),
        );
        let accelerating = POSITION_1.replace("-26.53", "-326.53");
        let findings = detector.evaluate(&parse_telemetry(&accelerating)).findings;
        assert!(kinds(&findings).contains(&AnomalyKind::HarshAcceleration));
    }

    #[test]
    fn small_push_is_not_a_harsh_event() {
        let mut detector = detector();
        let push = POSITION_1.replace("4.51, -26.53", "104.51, -26.53");
        let findings = detector.evaluate(&parse_telemetry(&push)).findings;
        assert!(!kinds(&findings).contains(&AnomalyKind::HarshAcceleration));
        assert!(!kinds(&findings).contains(&AnomalyKind::HarshBraking));
    }

    #[test]
    fn parses_all_signals() {
        let sample = parse_telemetry(POSITION_1);
        assert_eq!(sample.len(), 9);
        assert_eq!(sample["pressure"], 965.65);
        assert_eq!(sample["accel_y"], -26.53);
        assert_eq!(sample["mag_z"], 586.5);
    }

    #[test]
    fn skips_malformed_lines() {
        let sample = parse_telemetry("Pressure: abc\nAcceleration: 1, 2\nFoo: 1\nHumidity: 50.0");
        assert_eq!(sample.len(), 1);
        assert_eq!(sample["humidity"], 50.0);
    }

    #[test]
    fn trains_all_modes_and_signals_from_baseline() {
        let model = Model::train_from_csv(BASELINE).unwrap();
        let names: Vec<&str> = model.modes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["position_1", "position_2"]);
        for mode in &model.modes {
            assert_eq!(mode.signals.len(), 9);
            assert!(mode.signals.iter().all(|s| s.std > 0.0 && s.delta_std > 0.0));
        }
    }

    #[test]
    fn trains_without_mode_column() {
        let model = Model::train_from_csv("a,b\n1,2\n3,4\n").unwrap();
        assert_eq!(model.modes.len(), 1);
        assert_eq!(model.modes[0].name, DEFAULT_MODE);
    }

    #[test]
    fn rejects_inconsistent_training_data() {
        assert!(Model::train_from_csv("a,b\n1,2\n3\n").is_err());
        assert!(Model::train_from_csv("mode,a\nx,1\n").is_err());
        assert!(Model::train_from_csv("a,b\n").is_err());
        assert!(Model::train_from_csv("").is_err());
    }

    #[test]
    fn both_known_positions_are_normal() {
        let mut detector = detector();
        let evaluation = detector.evaluate(&parse_telemetry(POSITION_1));
        assert_eq!(evaluation.mode, "position_1");
        assert!(evaluation.findings.is_empty());

        let mut detector = self::detector();
        for telemetry in POSITION_2 {
            let evaluation = detector.evaluate(&parse_telemetry(telemetry));
            assert_eq!(evaluation.mode, "position_2");
            assert!(evaluation.findings.is_empty(), "{:?}", evaluation.findings);
        }
    }

    #[test]
    fn turning_the_device_is_a_sudden_change() {
        let mut detector = detector();
        detector.evaluate(&parse_telemetry(POSITION_1));
        let evaluation = detector.evaluate(&parse_telemetry(POSITION_2[0]));
        assert_eq!(evaluation.mode, "position_2");
        assert!(evaluation.findings.iter().all(|f| f.kind == AnomalyKind::SuddenChange));
        assert!(evaluation.findings.iter().any(|f| f.signal == "mag_z"));
    }

    #[test]
    fn tilting_the_device_is_detected() {
        let mut detector = detector();
        detector.evaluate(&parse_telemetry(POSITION_1));
        let tilted = POSITION_1.replace("4.51, -26.53, 1023.89", "-73.93, -490.87, 881.76");
        let findings = detector.evaluate(&parse_telemetry(&tilted)).findings;
        assert!(findings.iter().any(|f| f.signal == "accel_y" && f.kind == AnomalyKind::OutOfRange));
        assert!(findings.iter().any(|f| f.signal == "accel_y" && f.kind == AnomalyKind::SuddenChange));
    }

    #[test]
    fn unknown_heading_is_out_of_range() {
        let mut detector = detector();
        let turned = POSITION_1.replace("616.50, 301.50, 586.50", "-300.00, 500.00, 100.00");
        let findings = detector.evaluate(&parse_telemetry(&turned)).findings;
        assert!(findings.iter().any(|f| f.signal == "mag_x" && f.kind == AnomalyKind::OutOfRange));
    }

    #[test]
    fn humidity_jump_within_range_is_a_sudden_change() {
        let mut detector = detector();
        detector.evaluate(&parse_telemetry(POSITION_1));
        let breath = POSITION_1.replace("Humidity: 47.53", "Humidity: 57.53");
        let findings = detector.evaluate(&parse_telemetry(&breath)).findings;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].signal, "humidity");
        assert_eq!(findings[0].kind, AnomalyKind::SuddenChange);
    }

    #[test]
    fn first_sample_is_never_a_sudden_change() {
        let mut detector = detector();
        let hot = POSITION_1.replace("Temperature: 31.16", "Temperature: 45.00");
        let findings = detector.evaluate(&parse_telemetry(&hot)).findings;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, AnomalyKind::OutOfRange);
    }
}
