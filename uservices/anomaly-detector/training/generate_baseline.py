#!/usr/bin/env python3
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Generates the baseline training data for the anomaly detector: an MXChip AZ3166 lying still on a desk
# in each known position, sampled every 5 s. Each signal = reference value + slow drift (bounded random
# walk) + sensor noise. The magnetometer depends on the heading, so every position is its own mode.
# Usage: python3 generate_baseline.py > az3166-baseline.csv

import csv
import random
import sys

SAMPLES_PER_POSITION = 720  # one hour at 5 s per sample
SEED = 16

# reference values per position, taken from real telemetry
POSITIONS = {
    "position_1": {
        "pressure": 965.65,
        "temperature": 31.16,
        "humidity": 47.53,
        "accel_x": 4.51,
        "accel_y": -26.53,
        "accel_z": 1023.89,
        "mag_x": 616.50,
        "mag_y": 301.50,
        "mag_z": 586.50,
    },
    "position_2": {
        "pressure": 965.53,
        "temperature": 31.50,
        "humidity": 48.54,
        "accel_x": 9.69,
        "accel_y": -18.39,
        "accel_z": 1021.22,
        "mag_x": 415.88,
        "mag_y": 270.38,
        "mag_z": -205.13,
    },
}

# signal: (max drift, drift step, noise)
# environment drifts slowly (warm-up, weather); acceleration/magnetic only jitter while the device lies still
VARIATION = {
    "pressure": (2.0, 0.05, 0.05),  # hPa
    "temperature": (3.0, 0.10, 0.05),  # degC
    "humidity": (8.0, 0.30, 0.30),  # %RH
    "accel_x": (10.0, 0.5, 8.0),  # mg
    "accel_y": (10.0, 0.5, 8.0),
    "accel_z": (10.0, 0.5, 8.0),
    "mag_x": (15.0, 0.8, 6.0),  # mGauss
    "mag_y": (15.0, 0.8, 6.0),
    "mag_z": (15.0, 0.8, 6.0),
}


def main():
    rng = random.Random(SEED)
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(["mode", *VARIATION.keys()])
    for position, references in POSITIONS.items():
        drift = {name: 0.0 for name in VARIATION}
        for _ in range(SAMPLES_PER_POSITION):
            row = [position]
            for name, (max_drift, step, noise) in VARIATION.items():
                drift[name] = max(-max_drift, min(max_drift, drift[name] + rng.gauss(0.0, step)))
                row.append(f"{references[name] + drift[name] + rng.gauss(0.0, noise):.2f}")
            writer.writerow(row)


if __name__ == "__main__":
    main()
