#!/usr/bin/env python3
#
# SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
# SPDX-License-Identifier: Apache-2.0
#
# AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
#
# Converts a telemetry recording into training data for the anomaly detector.
# Record on board A (one JSON object per message):
#   docker exec sdv-imx95-a-mosquitto-1 mosquitto_sub -h 127.0.0.1 -t 'ThreadXAZ3166/telemetry' -F '%j' -W 300 > rec.jsonl
# Convert:
#   python3 recording_to_csv.py rec.jsonl [mode] > az3166-recorded.csv

import csv
import json
import sys
from datetime import datetime

SIGNALS = {
    "Pressure": ["pressure"],
    "Temperature": ["temperature"],
    "Humidity": ["humidity"],
    "Acceleration": ["accel_x", "accel_y", "accel_z"],
    "Magnetic": ["mag_x", "mag_y", "mag_z"],
}
COLUMNS = [name for names in SIGNALS.values() for name in names]


def parse(payload):
    sample = {}
    for line in payload.splitlines():
        key, _, values = line.partition(":")
        names = SIGNALS.get(key.strip())
        if not names:
            continue
        try:
            parsed = [float(v) for v in values.split(",")]
        except ValueError:
            continue
        if len(parsed) == len(names):
            sample.update(zip(names, parsed))
    return sample


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__ or "usage: recording_to_csv.py <recording.jsonl> [mode]")
    mode = sys.argv[2] if len(sys.argv) > 2 else "recorded"
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(["mode", *COLUMNS])
    timestamps, skipped = [], 0
    with open(sys.argv[1], encoding="utf-8") as recording:
        for line in recording:
            if not line.strip():
                continue
            message = json.loads(line)
            sample = parse(message.get("payload", ""))
            if len(sample) != len(COLUMNS):
                skipped += 1
                continue
            writer.writerow([mode, *(f"{sample[c]:.2f}" for c in COLUMNS)])
            timestamps.append(datetime.strptime(message["tst"], "%Y-%m-%dT%H:%M:%S.%f%z"))
    intervals = [(b - a).total_seconds() for a, b in zip(timestamps, timestamps[1:])]
    average = sum(intervals) / len(intervals) if intervals else 0.0
    print(f"{len(timestamps)} samples, {skipped} incomplete skipped, "
          f"average interval {average:.2f} s", file=sys.stderr)


if __name__ == "__main__":
    main()
