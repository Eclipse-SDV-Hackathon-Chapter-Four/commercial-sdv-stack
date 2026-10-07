/*
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
 *
 * Portions of this file were generated with AI assistance (GitHub Copilot).
 */
"use strict";

const MAX_EVENTS = 2000;
const MAX_ROWS = 500;

const state = {
    events: [],
    topicCounts: new Map(),
    received: 0,
    paused: false,
    selectedId: null,
    topicFilter: "",
    textFilter: "",
    pending: [],
    renderScheduled: false,
    localUri: null,
};

const $ = (id) => document.getElementById(id);
const rows = $("event-rows");

// ---------- filtering ----------

/** Matches an MQTT topic against a filter that may contain the wildcards '+' and '#'. */
function mqttMatches(filter, topic) {
    const f = filter.split("/");
    const t = topic.split("/");
    for (let i = 0; i < f.length; i++) {
        if (f[i] === "#") return true;
        if (i >= t.length) return false;
        if (f[i] !== "+" && f[i] !== t[i]) return false;
    }
    return f.length === t.length;
}

function topicMatches(topic) {
    const filter = state.topicFilter;
    if (!filter) return true;
    if (filter.includes("+") || filter.includes("#")) return mqttMatches(filter, topic);
    return topic.toLowerCase().includes(filter.toLowerCase());
}

function textMatches(ev) {
    const q = state.textFilter.toLowerCase();
    if (!q) return true;
    const haystack = [
        ev.topic,
        ev.payloadText,
        ev.payloadHex,
        ev.contentType,
        ev.responseTopic,
        ...(ev.userProperties || []).flatMap((p) => [p.key, p.value]),
        ...Object.values(ev.uprotocol || {}).map((v) => (v == null ? null : String(v))),
    ];
    return haystack.some((s) => s && s.toLowerCase().includes(q));
}

const visible = (ev) => topicMatches(ev.topic) && textMatches(ev);

// ---------- rendering ----------

const timeFmt = new Intl.DateTimeFormat(undefined, {
    hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3, hour12: false,
});

function formatSize(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

function payloadPreview(ev) {
    switch (ev.payloadFormat) {
        case "EMPTY": return "(empty)";
        case "BINARY": return ev.payloadHex;
        default: return ev.payloadText.replace(/\s+/g, " ");
    }
}

function cell(text, className) {
    const td = document.createElement("td");
    td.textContent = text;
    if (className) td.className = className;
    return td;
}

function buildRow(ev) {
    const tr = document.createElement("tr");
    tr.dataset.id = ev.id;
    const flags = ev.uprotocol
        ? ev.uprotocol.type
        : `${ev.qos}${ev.retained ? " R" : ""}${ev.duplicate ? " D" : ""}`;
    tr.append(
        cell(ev.id),
        cell(timeFmt.format(new Date(ev.receivedAt))),
        cell(ev.topic),
        cell(flags),
        cell(formatSize(ev.payloadSize)),
        cell(payloadPreview(ev), `payload ${ev.payloadFormat.toLowerCase()}`),
    );
    tr.children[2].title = ev.topic;
    if (isUProtocol(ev)) {
        const tag = document.createElement("span");
        tag.className = "tag up";
        tag.textContent = "uP";
        tag.title = "uProtocol message";
        tr.children[2].prepend(tag, " ");
        if (ev.uprotocol?.direction === "SENT") {
            const sent = document.createElement("span");
            sent.className = "tag sent";
            sent.textContent = "sent";
            sent.title = "sent by this viewer";
            tr.children[2].prepend(sent, " ");
        }
    }
    if (ev.id === state.selectedId) tr.classList.add("selected");
    return tr;
}

function renderAll() {
    const shown = state.events.filter(visible).slice(-MAX_ROWS).reverse();
    rows.replaceChildren(...shown.map(buildRow));
    updateCounters();
    renderTopics();
}

function flushPending() {
    state.renderScheduled = false;
    if (state.paused) return;
    const fresh = state.pending.splice(0).filter(visible);
    if (fresh.length) {
        const fragment = document.createDocumentFragment();
        for (let i = fresh.length - 1; i >= 0; i--) {
            const tr = buildRow(fresh[i]);
            tr.classList.add("fresh");
            fragment.append(tr);
        }
        rows.prepend(fragment);
        while (rows.children.length > MAX_ROWS) rows.lastElementChild.remove();
    }
    updateCounters();
    renderTopics();
}

function scheduleRender() {
    if (!state.renderScheduled) {
        state.renderScheduled = true;
        requestAnimationFrame(flushPending);
    }
}

function updateCounters() {
    $("count-received").textContent = state.received;
    $("count-shown").textContent = rows.children.length;
    $("empty").hidden = rows.children.length > 0;
}

function renderTopics() {
    const items = [...state.topicCounts.entries()].sort(([a], [b]) => a.localeCompare(b));
    $("topic-list").replaceChildren(...items.map(([topic, count]) => {
        const li = document.createElement("li");
        const name = document.createElement("span");
        name.textContent = topic;
        const badge = document.createElement("span");
        badge.className = "count";
        badge.textContent = count;
        li.append(name, badge);
        li.title = topic;
        if (topic === state.topicFilter) li.classList.add("selected");
        li.addEventListener("click", () => {
            setTopicFilter(state.topicFilter === topic ? "" : topic);
        });
        return li;
    }));
}

function setConnection(conn) {
    const badge = $("connection");
    badge.className = `badge ${conn.connected ? "connected" : "disconnected"}`;
    badge.textContent = conn.connected ? `connected · ${conn.brokerUri}` : `disconnected · ${conn.brokerUri}`;
    badge.title = [
        state.localUri ? `uEntity: ${state.localUri}` : null,
        `client ID: ${conn.clientId ?? "-"}`,
        `${state.localUri ? "listeners" : "topics"}: ${(conn.topics || []).join(", ")}`,
        conn.lastError ? `last error: ${conn.lastError}` : null,
    ].filter(Boolean).join("\n");
}

// ---------- uProtocol ----------

// see https://github.com/eclipse-uprotocol/up-spec/blob/main/up-l1/mqtt_5.adoc
const UP_ATTRIBUTES = {
    uP: "uProtocol version", 1: "id", 2: "type", 3: "source", 4: "sink", 5: "priority", 6: "ttl",
    7: "permission level", 8: "commstatus", 10: "token", 11: "traceparent",
};
const UP_MESSAGE_TYPES = { 1: "publish", 2: "request", 3: "response", 4: "notification" };
const UP_PAYLOAD_FORMATS = {
    0: "UNSPECIFIED", 1: "PROTOBUF_WRAPPED_IN_ANY", 2: "PROTOBUF", 3: "JSON", 4: "SOMEIP", 5: "SOMEIP_TLV",
    6: "RAW", 7: "TEXT", 8: "SHM",
};

const isUProtocol = (ev) => Boolean(ev.uprotocol) || (ev.userProperties || []).some((p) => p.key === "uP");

// attributes of messages received via the uProtocol transport, in display order
const UP_DECODED_ATTRIBUTES = [
    ["direction", "direction"], ["type", "type"], ["id", "id"], ["createdAt", "created at (from id)"],
    ["source", "source"], ["sink", "sink"], ["priority", "priority"], ["ttl", "ttl (ms)"],
    ["permissionLevel", "permission level"], ["commStatus", "commstatus"], ["reqId", "reqid"],
    ["token", "token"], ["traceparent", "traceparent"], ["payloadFormat", "payload format"],
];

function uProtocolLabel(key, value) {
    const name = UP_ATTRIBUTES[key];
    if (!name) return [key, value];
    if (key === "2" && UP_MESSAGE_TYPES[value]) value = `${value} (${UP_MESSAGE_TYPES[value]})`;
    return [`${key} · ${name}`, value];
}

function formatUuid(hex) {
    return hex && hex.length === 32
        ? `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
        : hex;
}

// ---------- details panel ----------

function showDetails(id) {
    const ev = state.events.find((e) => e.id === id);
    if (!ev) return;
    state.selectedId = id;
    rows.querySelectorAll("tr.selected").forEach((tr) => tr.classList.remove("selected"));
    rows.querySelector(`tr[data-id="${id}"]`)?.classList.add("selected");

    $("details").hidden = false;
    $("details-title").textContent = ev.topic;
    const up = isUProtocol(ev);
    const contentType = up && UP_PAYLOAD_FORMATS[ev.contentType]
        ? `${ev.contentType} (payload format ${UP_PAYLOAD_FORMATS[ev.contentType]})`
        : ev.contentType;

    const meta = ev.uprotocol ? [
        ["Message #", ev.id],
        ["Received", new Date(ev.receivedAt).toISOString()],
        ["Size", `${ev.payloadSize} bytes${ev.payloadTruncated ? " (preview truncated)" : ""}`],
    ] : [
        ["Message #", ev.id],
        ["Received", new Date(ev.receivedAt).toISOString()],
        ["QoS", ev.qos],
        ["Retained", ev.retained],
        ["Duplicate", ev.duplicate],
        ["Size", `${ev.payloadSize} bytes${ev.payloadTruncated ? " (preview truncated)" : ""}`],
        ["Content type", contentType],
        ["Response topic", ev.responseTopic],
        up ? ["Correlation data · reqId", formatUuid(ev.correlationDataHex)] : ["Correlation data", ev.correlationDataHex],
        ["Message expiry", ev.messageExpiryInterval != null ? `${ev.messageExpiryInterval} s` : null],
    ].filter(([, v]) => v !== null && v !== undefined);
    $("details-meta").replaceChildren(...meta.flatMap(([k, v]) => {
        const dt = document.createElement("dt");
        dt.textContent = k;
        const dd = document.createElement("dd");
        dd.textContent = String(v);
        return [dt, dd];
    }));

    $("details-format").textContent = ev.payloadFormat;
    let payload;
    switch (ev.payloadFormat) {
        case "EMPTY": payload = "(empty)"; break;
        case "BINARY": payload = ev.payloadHex; break;
        case "JSON":
            try {
                payload = JSON.stringify(JSON.parse(ev.payloadText), null, 2);
            } catch {
                payload = ev.payloadText;
            }
            break;
        default: payload = ev.payloadText;
    }
    $("details-payload").textContent = payload;

    $("details-base64-block").hidden = !ev.payloadBase64;
    $("details-base64").textContent = ev.payloadBase64 ?? "";

    const props = ev.uprotocol
        ? UP_DECODED_ATTRIBUTES
            .filter(([key]) => ev.uprotocol[key] != null)
            .map(([key, label]) => ({ key: label, value: String(ev.uprotocol[key]) }))
        : (ev.userProperties || []);
    $("details-user-props-title").textContent = ev.uprotocol ? "uProtocol attributes" : "User properties";
    $("details-user-props-block").hidden = props.length === 0;
    $("details-user-props").replaceChildren(...props.map((p) => {
        const [key, value] = up && !ev.uprotocol ? uProtocolLabel(p.key, p.value) : [p.key, p.value];
        const tr = document.createElement("tr");
        tr.append(cell(key), cell(value));
        return tr;
    }));
}

function hideDetails() {
    state.selectedId = null;
    $("details").hidden = true;
    rows.querySelectorAll("tr.selected").forEach((tr) => tr.classList.remove("selected"));
}

// ---------- data handling ----------

function addEvent(ev) {
    state.events.push(ev);
    if (state.events.length > MAX_EVENTS) state.events.splice(0, state.events.length - MAX_EVENTS);
    state.topicCounts.set(ev.topic, (state.topicCounts.get(ev.topic) || 0) + 1);
    state.received++;
}

function reset(history) {
    state.events = [];
    state.pending = [];
    state.topicCounts.clear();
    state.received = 0;
    history.forEach(addEvent);
    if (state.selectedId !== null && !state.events.some((e) => e.id === state.selectedId)) hideDetails();
    renderAll();
}

function connect() {
    const source = new EventSource("api/events/stream");
    source.addEventListener("reset", (e) => reset(JSON.parse(e.data)));
    source.addEventListener("status", (e) => setConnection(JSON.parse(e.data)));
    source.addEventListener("mqtt", (e) => {
        const ev = JSON.parse(e.data);
        addEvent(ev);
        state.pending.push(ev);
        scheduleRender();
    });
    source.onerror = () => {
        const badge = $("connection");
        badge.className = "badge disconnected";
        badge.textContent = "viewer backend unreachable – retrying…";
    };
}

// ---------- controls ----------

function setTopicFilter(value) {
    state.topicFilter = value;
    $("topic-filter").value = value;
    renderAll();
}

$("topic-filter").addEventListener("input", (e) => {
    state.topicFilter = e.target.value.trim();
    renderAll();
});

$("text-filter").addEventListener("input", (e) => {
    state.textFilter = e.target.value.trim();
    renderAll();
});

$("pause").addEventListener("click", (e) => {
    state.paused = !state.paused;
    e.target.textContent = state.paused ? "Resume" : "Pause";
    e.target.classList.toggle("active", state.paused);
    if (!state.paused) {
        state.pending = [];
        renderAll();
    }
});

$("clear").addEventListener("click", async () => {
    await fetch("api/events", { method: "DELETE" });
});

rows.addEventListener("click", (e) => {
    const tr = e.target.closest("tr");
    if (tr) showDetails(Number(tr.dataset.id));
});

$("details-close").addEventListener("click", hideDetails);
document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideDetails();
});

// ---------- uProtocol RPC ----------

async function initUProtocol() {
    const response = await fetch("api/uprotocol").catch(() => null);
    if (!response?.ok) return;
    const info = await response.json();
    state.localUri = info.localUri;
    $("rpc-source").textContent = `${info.localUri} →`;
    $("rpc-toggle").hidden = !info.rpcEnabled;
}

$("rpc-toggle").addEventListener("click", (e) => {
    const form = $("rpc");
    form.hidden = !form.hidden;
    e.target.classList.toggle("active", !form.hidden);
    if (!form.hidden) $("rpc-method").focus();
});

$("rpc").addEventListener("submit", async (e) => {
    e.preventDefault();
    const result = $("rpc-result");
    result.className = "rpc-result";
    result.textContent = "invoking…";
    try {
        const response = await fetch("api/uprotocol/rpc", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
                method: $("rpc-method").value.trim(),
                payload: $("rpc-payload").value || null,
                token: $("rpc-token").value.trim() || null,
            }),
        });
        const outcome = await response.json();
        const ok = outcome.status === "OK";
        result.classList.add(ok ? "ok" : "failed");
        result.textContent = [
            outcome.status,
            outcome.durationMillis != null ? `${outcome.durationMillis} ms` : null,
            outcome.message,
            outcome.payloadText ?? outcome.payloadHex,
        ].filter(Boolean).join(" · ");
    } catch (err) {
        result.classList.add("failed");
        result.textContent = `request failed: ${err}`;
    }
});

initUProtocol().finally(connect);
