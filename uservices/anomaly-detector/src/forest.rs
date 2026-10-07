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

//! Isolation Forest (Extended Isolation Forest, Hariri et al., IEEE TKDE 2019) trained on sliding
//! window features of recorded normal telemetry. The features compare each sample with the
//! previous samples, so they do not depend on how the device is oriented.

use std::collections::VecDeque;

use extended_isolation_forest::{Forest, ForestOptions};

const FEATURE_COUNT: usize = 5;
pub(crate) const FEATURES: [&str; FEATURE_COUNT] = [
    "acceleration change",
    "acceleration direction change",
    "acceleration magnitude change",
    "temperature rate",
    "temperature deviation",
];
const FEATURE_UNITS: [&str; FEATURE_COUNT] = ["mg", "°", "mg", "°C", "°C"];
// keeps nearly constant features (e.g. temperature at rest) from dominating after scaling
const MIN_FEATURE_STD: [f64; FEATURE_COUNT] = [2.0, 0.2, 2.0, 0.05, 0.05];
/// Number of previous samples the features are computed against (~30 s at 5 s per sample).
pub(crate) const WINDOW: usize = 6;
const MIN_TRAINING_VECTORS: usize = 32;
// how far above the highest training score a sample has to be to count as an anomaly
const THRESHOLD_MARGIN: f64 = 0.02;

type Features = [f64; FEATURE_COUNT];

/// Computes orientation-independent features of a sample relative to a sliding window.
#[derive(Default)]
pub(crate) struct FeatureExtractor {
    window: VecDeque<([f64; 3], f64)>,
}

impl FeatureExtractor {
    pub(crate) fn reset(&mut self) {
        self.window.clear();
    }

    /// Adds a sample and returns its features, once enough previous samples are available.
    pub(crate) fn push(&mut self, accel: [f64; 3], temperature: f64) -> Option<Features> {
        let features = (self.window.len() >= 2).then(|| self.features(accel, temperature));
        if self.window.len() == WINDOW {
            self.window.pop_front();
        }
        self.window.push_back((accel, temperature));
        features
    }

    fn features(&self, accel: [f64; 3], temperature: f64) -> Features {
        let n = self.window.len() as f64;
        let mut mean_accel = [0.0; 3];
        for (sample, _) in &self.window {
            for axis in 0..3 {
                mean_accel[axis] += sample[axis] / n;
            }
        }
        let mean_magnitude = self.window.iter().map(|(a, _)| norm(*a)).sum::<f64>() / n;
        let mean_temperature = self.window.iter().map(|(_, t)| t).sum::<f64>() / n;
        let last_temperature = self.window.back().map(|(_, t)| *t).unwrap_or(temperature);

        let change = norm([
            accel[0] - mean_accel[0],
            accel[1] - mean_accel[1],
            accel[2] - mean_accel[2],
        ]);
        let cosine = (accel[0] * mean_accel[0] + accel[1] * mean_accel[1] + accel[2] * mean_accel[2])
            / (norm(accel) * norm(mean_accel)).max(f64::EPSILON);
        [
            change,
            cosine.clamp(-1.0, 1.0).acos().to_degrees(),
            (norm(accel) - mean_magnitude).abs(),
            (temperature - last_temperature).abs(),
            (temperature - mean_temperature).abs(),
        ]
    }
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The most unusual feature of a sample, for explaining an anomaly.
pub(crate) struct Cause {
    pub feature: &'static str,
    pub value: f64,
    pub unit: &'static str,
    pub training_max: f64,
}

pub(crate) struct IsolationForestModel {
    forest: Forest<f64, FEATURE_COUNT>,
    mean: Features,
    std: Features,
    training_max: Features,
    pub threshold: f64,
    pub training_vectors: usize,
    pub max_training_score: f64,
}

impl IsolationForestModel {
    /// Trains the forest on consecutive samples of normal operation (acceleration in mg, temperature).
    pub(crate) fn train(samples: &[([f64; 3], f64)], min_threshold: f64) -> Result<Self, String> {
        let mut extractor = FeatureExtractor::default();
        let vectors: Vec<Features> = samples
            .iter()
            .filter_map(|(accel, temperature)| extractor.push(*accel, *temperature))
            .collect();
        if vectors.len() < MIN_TRAINING_VECTORS {
            return Err(format!(
                "{} training samples are too few, at least {} are needed",
                samples.len(),
                MIN_TRAINING_VECTORS + 2
            ));
        }
        let n = vectors.len() as f64;
        let mut mean = [0.0; FEATURE_COUNT];
        let mut std = [0.0; FEATURE_COUNT];
        let mut training_max = [0.0_f64; FEATURE_COUNT];
        for i in 0..FEATURE_COUNT {
            mean[i] = vectors.iter().map(|v| v[i]).sum::<f64>() / n;
            let variance = vectors.iter().map(|v| (v[i] - mean[i]).powi(2)).sum::<f64>() / n;
            std[i] = variance.sqrt().max(MIN_FEATURE_STD[i]);
            training_max[i] = vectors.iter().map(|v| v[i]).fold(0.0, f64::max);
        }
        let scaled: Vec<Features> = vectors.iter().map(|v| scale(v, &mean, &std)).collect();
        let options = ForestOptions {
            n_trees: 200,
            sample_size: scaled.len().min(64),
            max_tree_depth: None,
            extension_level: FEATURE_COUNT - 1,
        };
        let forest = Forest::from_slice(&scaled, &options).map_err(|e| e.to_string())?;
        let max_training_score = scaled.iter().map(|v| forest.score(v)).fold(0.0, f64::max);
        Ok(Self {
            forest,
            mean,
            std,
            training_max,
            threshold: min_threshold.max(max_training_score + THRESHOLD_MARGIN),
            training_vectors: vectors.len(),
            max_training_score,
        })
    }

    /// Anomaly score between 0 and 1; normal data scores around 0.5 or lower.
    pub(crate) fn score(&self, features: &Features) -> f64 {
        self.forest.score(&scale(features, &self.mean, &self.std))
    }

    pub(crate) fn cause(&self, features: &Features) -> Cause {
        let index = (0..FEATURE_COUNT)
            .max_by(|&a, &b| {
                let za = (features[a] - self.mean[a]) / self.std[a];
                let zb = (features[b] - self.mean[b]) / self.std[b];
                za.total_cmp(&zb)
            })
            .unwrap_or(0);
        Cause {
            feature: FEATURES[index],
            value: features[index],
            unit: FEATURE_UNITS[index],
            training_max: self.training_max[index],
        }
    }
}

fn scale(features: &Features, mean: &Features, std: &Features) -> Features {
    let mut scaled = [0.0; FEATURE_COUNT];
    for i in 0..FEATURE_COUNT {
        // rounding avoids values that differ only by float error: rand 0.8's Uniform::new, used by
        // the forest, loops (almost) forever on such bounds
        scaled[i] = ((features[i] - mean[i]) / std[i] * 100.0).round() / 100.0;
    }
    scaled
}

/// Reads consecutive training samples from CSV with (at least) the columns accel_x, accel_y,
/// accel_z and temperature, as written by training/recording_to_csv.py.
pub(crate) fn read_training_csv(csv: &str) -> Result<Vec<([f64; 3], f64)>, String> {
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<&str> = lines
        .next()
        .ok_or("training data is empty")?
        .split(',')
        .map(str::trim)
        .collect();
    let column = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .ok_or(format!("training data has no column {name}"))
    };
    let columns = [
        column("accel_x")?,
        column("accel_y")?,
        column("accel_z")?,
        column("temperature")?,
    ];
    lines
        .enumerate()
        .map(|(index, line)| {
            let values: Vec<&str> = line.split(',').map(str::trim).collect();
            let value = |column: usize| {
                values
                    .get(column)
                    .ok_or(format!("row {}: missing value", index + 2))?
                    .parse::<f64>()
                    .map_err(|e| format!("row {}: {e}", index + 2))
            };
            Ok((
                [value(columns[0])?, value(columns[1])?, value(columns[2])?],
                value(columns[3])?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo random jitter in [-1, 1].
    fn jitter(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
    }

    /// The device lying still: a few mg of noise around gravity, slowly warming up.
    fn resting(count: usize) -> Vec<([f64; 3], f64)> {
        let mut seed = 16;
        (0..count)
            .map(|i| {
                (
                    [
                        -9.0 + 2.0 * jitter(&mut seed),
                        -14.3 + 2.0 * jitter(&mut seed),
                        1022.5 + 1.5 * jitter(&mut seed),
                    ],
                    28.5 + i as f64 * 0.005 + 0.03 * jitter(&mut seed),
                )
            })
            .collect()
    }

    fn trained() -> (IsolationForestModel, FeatureExtractor) {
        let samples = resting(120);
        let model = IsolationForestModel::train(&samples, 0.6).unwrap();
        let mut extractor = FeatureExtractor::default();
        for (accel, temperature) in &samples[samples.len() - WINDOW..] {
            extractor.push(*accel, *temperature);
        }
        (model, extractor)
    }

    #[test]
    fn features_do_not_depend_on_orientation() {
        let mut extractor = FeatureExtractor::default();
        // gravity along a tilted axis, device at rest
        let tilted = [0.0, 707.1, 707.1];
        for _ in 0..WINDOW {
            extractor.push(tilted, 25.0);
        }
        let features = extractor.push(tilted, 25.0).unwrap();
        assert!(features.iter().all(|f| f.abs() < 1e-6), "{features:?}");
    }

    #[test]
    fn rejects_too_little_training_data() {
        assert!(IsolationForestModel::train(&resting(10), 0.6).is_err());
    }

    #[test]
    fn resting_device_is_normal() {
        let (model, mut extractor) = trained();
        let (accel, temperature) = resting(121)[120];
        let features = extractor.push(accel, temperature).unwrap();
        assert!(model.score(&features) <= model.threshold);
    }

    #[test]
    fn jolt_is_an_anomaly_caused_by_acceleration() {
        let (model, mut extractor) = trained();
        let features = extractor.push([-259.0, -14.3, 1022.5], 29.1).unwrap();
        assert!(model.score(&features) > model.threshold);
        assert!(model.cause(&features).feature.starts_with("acceleration"));
    }

    #[test]
    fn temperature_jump_is_an_anomaly_caused_by_temperature() {
        let (model, mut extractor) = trained();
        let features = extractor.push([-9.0, -14.3, 1022.5], 33.0).unwrap();
        assert!(model.score(&features) > model.threshold);
        assert!(model.cause(&features).feature.starts_with("temperature"));
    }

    #[test]
    fn trains_on_recorded_data() {
        let samples = read_training_csv(include_str!("../training/az3166-normal.csv")).unwrap();
        let model = IsolationForestModel::train(&samples, 0.6).unwrap();
        assert!(model.training_vectors > 100);
    }

    #[test]
    fn reads_recorded_csv() {
        let csv = "mode,pressure,temperature,humidity,accel_x,accel_y,accel_z,mag_x,mag_y,mag_z\n\
                   recorded,965.0,28.5,50.0,-9.0,-14.3,1022.5,10.0,250.0,-460.0\n";
        assert_eq!(read_training_csv(csv).unwrap(), [([-9.0, -14.3, 1022.5], 28.5)]);
        assert!(read_training_csv("a,b\n1,2\n").is_err());
    }
}
