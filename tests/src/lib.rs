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

//! Helpers for the Commercial SDV Stack authorization integration tests.
//!
//! The tests drive the CDA SOVD HTTP API (`:20002`) with JWT-SVIDs minted from
//! the running `spire-server`, proving that valid attested workloads succeed and
//! unauthorized / forbidden / expired / forged credentials all fail closed.

use std::{process::Command, sync::Once, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Method;

// ---------------------------------------------------------------------------
// Configuration (override via environment)
// ---------------------------------------------------------------------------

fn env_or(key: &str, default: impl Into<String>) -> String {
    std::env::var(key).unwrap_or_else(|_| default.into())
}

/// Directory holding `docker-compose.yaml`. Defaults to the crate's parent
/// (the commercial-sdv-stack root); override with `STACK_DIR`.
pub fn stack_dir() -> String {
    std::env::var("STACK_DIR").unwrap_or_else(|_| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate has a parent dir")
            .to_string_lossy()
            .into_owned()
    })
}

pub fn trust_domain() -> String {
    env_or("TRUST_DOMAIN", "sdv.eclipse.org")
}
pub fn spiffe_pmc() -> String {
    env_or(
        "SPIFFE_PMC",
        format!("spiffe://{}/vehicle/powertrain-mode-controller", trust_domain()),
    )
}
pub fn spiffe_properties() -> String {
    env_or("SPIFFE_PROPERTIES", format!("spiffe://{}/vehicle/properties", trust_domain()))
}
pub fn spiffe_unknown() -> String {
    env_or("SPIFFE_UNKNOWN", format!("spiffe://{}/vehicle/intruder", trust_domain()))
}
pub fn aud_cda() -> String {
    env_or("AUD_CDA", "sovd.cda")
}
pub fn aud_wrong() -> String {
    env_or("AUD_WRONG", "wrong.audience")
}
pub fn cda_base() -> String {
    env_or("CDA_BASE", format!("http://localhost:{}/vehicle/v15", env_or("CDA_PORT", "20002")))
}
pub fn pwt_path() -> String {
    env_or("PWT_DATA_PATH", "components/blueprint-ecu/data/powertrain_mode")
}
pub fn pwt_write_body() -> String {
    env_or("PWT_WRITE_BODY", r#"{"data":{"Mode":"Economy"}}"#)
}

/// Seconds to wait past a token's `exp` before the expiry check. SPIRE's JWT
/// validation (go-jose) applies a ~60s clock-skew leeway, so a token must be
/// expired by MORE than that before it is rejected. Default 70s.
pub fn expiry_wait_secs() -> u64 {
    env_or("EXPIRY_WAIT_SECONDS", "70").parse().unwrap_or(70)
}

// ---------------------------------------------------------------------------
// SPIRE helpers
// ---------------------------------------------------------------------------

fn extract_jwt(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .find(|l| {
            l.starts_with("eyJ") && {
                let p: Vec<&str> = l.split('.').collect();
                p.len() == 3 && p.iter().all(|s| !s.is_empty())
            }
        })
        .unwrap_or("")
        .to_string()
}

fn spire_jwt_mint(args: &[&str]) -> String {
    let mut cmd = Command::new("docker");
    cmd.current_dir(stack_dir()).args([
        "compose",
        "exec",
        "-T",
        "spire-server",
        "/opt/spire/bin/spire-server",
        "jwt",
        "mint",
        "-socketPath",
        "/run/spire/server/private/api.sock",
    ]);
    cmd.args(args);
    let out = cmd.output().expect("failed to run `docker compose exec ... jwt mint`");
    extract_jwt(&out.stdout)
}

/// Mint a JWT-SVID. Minting is an admin operation on `spire-server`: it bypasses
/// workload attestation, which is exactly what we want here -- we exercise the
/// CDA's token *validation* and *authorization* layers, not attestation.
pub fn mint_jwt(spiffe_id: &str, audience: &str, ttl: &str) -> String {
    spire_jwt_mint(&["-spiffeID", spiffe_id, "-audience", audience, "-ttl", ttl])
}

/// Mint a JWT-SVID carrying multiple audiences.
pub fn mint_jwt_multi(spiffe_id: &str, audiences: &[&str], ttl: &str) -> String {
    let mut args = vec!["-spiffeID", spiffe_id, "-ttl", ttl];
    for a in audiences {
        args.push("-audience");
        args.push(a);
    }
    spire_jwt_mint(&args)
}

/// An unsigned `alg:none` token -- proves the CDA rejects algorithm-confusion.
pub fn forge_alg_none(spiffe_id: &str, audience: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    // exp far in the future so the ONLY reason to reject is the missing signature.
    let payload = format!(r#"{{"aud":["{audience}"],"sub":"{spiffe_id}","exp":4102444800}}"#);
    format!("{header}.{}.", URL_SAFE_NO_PAD.encode(payload.as_bytes()))
}

/// Same token with a MIDDLE signature character flipped -- proves the CDA
/// verifies the signature. NB: the LAST base64url char of a 64-byte ES256
/// signature carries only 2 significant bits, so flipping it is often a no-op.
pub fn tamper(token: &str) -> String {
    let (head, sig) = token.rsplit_once('.').expect("token has a signature segment");
    let mut chars: Vec<char> = sig.chars().collect();
    if chars.is_empty() {
        return token.to_string();
    }
    let pos = chars.len() / 2;
    chars[pos] = if chars[pos] == 'A' { 'B' } else { 'A' };
    format!("{head}.{}", chars.into_iter().collect::<String>())
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// Issue a request to the CDA and return the HTTP status code (0 if unreachable).
pub fn request(method: Method, path: &str, token: Option<&str>, body: Option<&str>) -> u16 {
    let url = format!("{}/{}", cda_base(), path);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("build http client");
    let mut rb = client.request(method, &url);
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    if let Some(b) = body {
        rb = rb.header(reqwest::header::CONTENT_TYPE, "application/json").body(b.to_string());
    }
    rb.send().map(|r| r.status().as_u16()).unwrap_or(0)
}

/// Force variant detection once, so reads/writes resolve an ECU variant.
/// Best-effort: failures here are environmental, not authorization failures.
pub fn ensure_variant() {
    static VARIANT: Once = Once::new();
    VARIANT.call_once(|| {
        let token = mint_jwt(&spiffe_pmc(), &aud_cda(), "120s");
        let _ = request(Method::PUT, "components/blueprint-ecu", Some(&token), None);
    });
}

// ---------------------------------------------------------------------------
// Assertions (ALLOW = 2xx, DENY = 401/403)
// ---------------------------------------------------------------------------

pub fn assert_allow(label: &str, code: u16) {
    assert!((200..300).contains(&code), "{label}: expected ALLOW (2xx), got {code}");
}

pub fn assert_deny(label: &str, code: u16) {
    assert!(code == 401 || code == 403, "{label}: expected DENY (401/403), got {code}");
}
