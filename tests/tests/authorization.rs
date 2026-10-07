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

/*
 * AI assistance: parts of this file were generated with Claude Code (Opus 4.8)
 * and reviewed and verified by the human contributor. All content is
 * contributed under the Apache-2.0 license declared above.
 */

//! Authorization integration matrix against the CDA SOVD API (over its Unix socket).
//!
//! Prerequisites (see tests/README.md):
//!   docker compose --profile infra --profile powertrain up -d --build
//!   scripts/register_workloads.sh
//!
//! Run with `./run.sh` (from the tests/ crate) — the CDA is socket-hardened, so
//! the tests run in a container that mounts the socket volume. The expired-JWT
//! test waits ~70s past expiry (EXPIRY_WAIT_SECONDS) to clear SPIRE's leeway.

use std::{thread::sleep, time::Duration};

use reqwest::Method;
use sdv_integration_tests::*;

// --- Core authorization matrix --------------------------------------------

#[test]
fn authorized_read_is_allowed() {
    ensure_variant();
    let token = mint_jwt(&spiffe_pmc(), &aud_cda(), "300s");
    assert_allow("authorized read (Powertrain_Mode_Read)", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn authorized_write_is_allowed() {
    ensure_variant();
    let token = mint_jwt(&spiffe_pmc(), &aud_cda(), "300s");
    // The socket-hardened CDA requires a SOVD lock to write (a lock-less PUT is
    // rejected with 409 `lock-required`), so acquire a lock, write, then release
    // it -- exactly as the PMC does.
    let lock = acquire_lock(&token).expect("acquire SOVD write lock");
    let code = request(Method::PUT, &pwt_path(), Some(&token), Some(&pwt_write_body()));
    release_lock(&token, &lock);
    assert_allow("authorized write (Powertrain_Mode_Write)", code);
}

#[test]
fn registered_but_forbidden_identity_is_denied() {
    ensure_variant(); // gate on a ready stack so a denial is 403, not a 404 on an unresolved resource
    // vehicle/properties has NO entry in the CDA allow-list -> every op denied.
    let token = mint_jwt(&spiffe_properties(), &aud_cda(), "300s");
    assert_deny("registered-but-forbidden identity", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn unknown_spiffe_id_is_denied() {
    // NB: both this and registered_but_forbidden_identity are admin-minted, which
    // bypasses attestation, so at the CDA they exercise the SAME "not in the OPA
    // allow-list -> 403" branch. The registered-vs-unregistered *attestation*
    // distinction is covered by modified_image_gets_no_identity in offline.rs.
    ensure_variant();
    let token = mint_jwt(&spiffe_unknown(), &aud_cda(), "300s");
    assert_deny("unknown SPIFFE ID", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn wrong_audience_is_denied() {
    ensure_variant();
    let token = mint_jwt(&spiffe_pmc(), &aud_wrong(), "300s");
    assert_deny("wrong JWT audience", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn expired_token_is_denied() {
    ensure_variant();
    // Must wait past SPIRE's ~60s clock-skew leeway (see EXPIRY_WAIT_SECONDS),
    // otherwise a just-expired token is still accepted.
    let token = mint_jwt(&spiffe_pmc(), &aud_cda(), "2s");
    sleep(Duration::from_secs(expiry_wait_secs()));
    assert_deny("expired JWT (>leeway)", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn missing_token_is_denied() {
    ensure_variant();
    // A missing token is the one case that returns 401 (NoTokenProvided); every
    // other token failure maps to 403. Assert the exact code to guard ordering.
    assert_unauthenticated("missing token", request(Method::GET, &pwt_path(), None, None));
}

// --- Token hardening (forged credentials) ---------------------------------

#[test]
fn tampered_signature_is_denied() {
    ensure_variant();
    let token = tamper(&mint_jwt(&spiffe_pmc(), &aud_cda(), "300s"));
    assert_deny("tampered signature", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn alg_none_token_is_denied() {
    ensure_variant();
    let token = forge_alg_none(&spiffe_pmc(), &aud_cda());
    assert_deny("alg:none forged token", request(Method::GET, &pwt_path(), Some(&token), None));
}

#[test]
fn multi_audience_token_is_allowed() {
    // Audience match is "contains", not "equals".
    ensure_variant();
    let token = mint_jwt_multi(&spiffe_pmc(), &[&aud_cda(), "other.service"], "300s");
    assert_allow("multi-audience token (contains sovd.cda)", request(Method::GET, &pwt_path(), Some(&token), None));
}
