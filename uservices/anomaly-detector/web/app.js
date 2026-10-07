/*
 * SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation
 * SPDX-License-Identifier: Apache-2.0
 * AI-generated (GitHub Copilot, Claude Opus 5.5) - issue 16
 */

"use strict";

const UNITS = {
  acceleration: "mg",
  longitudinal_accel: "mg",
  temperature: "°C",
  pressure: "hPa",
  humidity: "%",
  accel_x: "mg",
  accel_y: "mg",
  accel_z: "mg",
  mag_x: "mG",
  mag_y: "mG",
  mag_z: "mG",
};
const LABELS = {
  isolation_forest_score: "Isolation Forest score",
  acceleration: "acceleration vs. rest",
  longitudinal_accel: "longitudinal (+ accelerating, − braking)",
};
const DETECTORS = {
  isolation_forest: "Isolation Forest",
  rules: "Rules",
};
// the MXChip publishes every ~5 s
const STALE_MS = 15000;
const SVG_NS = "http://www.w3.org/2000/svg";

function element(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}

function format(value, signal) {
  if (value === null || value === undefined) return "–";
  return `${value.toFixed(2)} ${UNITS[signal] || ""}`.trim();
}

function svg(tag, attributes) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, value);
  return node;
}

function sparkline(view) {
  const width = 180;
  const height = 36;
  const values = view.history;
  const monitored = view.normal_min !== null;
  const low = Math.min(...values, ...(monitored ? [view.normal_min] : []));
  const high = Math.max(...values, ...(monitored ? [view.normal_max] : []));
  const span = high - low || 1;
  const y = (value) => height - 2 - ((value - low) / span) * (height - 4);
  const chart = svg("svg", { viewBox: `0 0 ${width} ${height}`, width, height });
  if (monitored) {
    chart.appendChild(
      svg("rect", {
        x: 0,
        width,
        y: y(view.normal_max),
        height: Math.max(1, y(view.normal_min) - y(view.normal_max)),
        class: "band",
      }),
    );
  }
  if (values.length > 1) {
    const points = values
      .map((value, i) => `${((i / (values.length - 1)) * width).toFixed(1)},${y(value).toFixed(1)}`)
      .join(" ");
    chart.appendChild(svg("polyline", { points, class: view.anomalous ? "line bad" : "line" }));
  }
  return chart;
}

function setBadge(text, state) {
  const badge = document.getElementById("status");
  badge.textContent = text;
  badge.className = `badge ${state}`;
}

function setPill(id, anomalous, known) {
  const pill = document.getElementById(id);
  pill.textContent = !known ? "–" : anomalous ? "anomaly" : "normal";
  pill.className = `pill ${!known ? "off" : anomalous ? "bad" : "ok"}`;
}

function renderCompare(status) {
  const score = status.signals.find((s) => s.name === "isolation_forest_score");
  setPill("forest", status.findings.some((f) => f.detector === "isolation_forest"), score !== undefined);
  setPill("rules", status.findings.some((f) => f.detector === "rules"), status.samples > 0);
  document.getElementById("forest-score").textContent = score
    ? `score ${score.value.toFixed(3)} / threshold ${score.normal_max.toFixed(3)}`
    : "warming up";
}

function render(status) {
  document.getElementById("source").textContent = status.source || "–";
  document.getElementById("samples").textContent = status.samples;
  document.getElementById("updated").textContent = status.updated
    ? new Date(status.updated).toLocaleTimeString()
    : "–";

  const stale = !status.updated || Date.now() - Date.parse(status.updated) > STALE_MS;
  if (stale) setBadge("NO TELEMETRY", "stale");
  else if (status.anomaly) setBadge("ANOMALY", "bad");
  else setBadge("NORMAL", "ok");
  renderCompare(status);

  const rows = status.signals.map((view) => {
    const monitored = view.normal_min !== null;
    const row = element("tr", undefined, monitored ? "" : "muted");
    if (view.anomalous) row.className = "bad";
    const state = element("td");
    if (!monitored) state.appendChild(element("span", "not monitored", "pill off"));
    else if (view.anomalous) state.appendChild(element("span", "anomaly", "pill bad"));
    else state.appendChild(element("span", "normal", "pill ok"));
    const chart = element("td");
    chart.appendChild(sparkline(view));
    row.append(
      element("td", LABELS[view.name] || view.name),
      element("td", format(view.value, view.name), "num"),
      element("td", monitored ? `${view.normal_min.toFixed(2)} … ${view.normal_max.toFixed(2)}` : "–", "num"),
      state,
      chart,
    );
    return row;
  });
  document.getElementById("signals").replaceChildren(...rows);

  const events = status.events.map((event) => {
    const row = element("tr");
    row.append(
      element("td", new Date(event.timestamp).toLocaleString()),
      element("td", DETECTORS[event.detector] || event.detector),
      element("td", event.cause),
    );
    return row;
  });
  document.getElementById("events").replaceChildren(...events);
  document.getElementById("no-events").hidden = events.length > 0;
}

async function refresh() {
  try {
    const response = await fetch("/api/status", { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    render(await response.json());
  } catch {
    setBadge("DISCONNECTED", "stale");
  }
}

refresh();
setInterval(refresh, 2000);
// browsers throttle timers in background tabs, so catch up as soon as the tab is shown again
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) refresh();
});
