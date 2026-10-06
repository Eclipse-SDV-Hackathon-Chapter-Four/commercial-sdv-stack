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

use std::sync::Arc;

use clap::Parser;
use common::AUDIENCE_SOVD_CDA;
use common::powertrain::{AUDIENCE_POWERTRAIN_MODE_CONTROL, ModeMessage};
use log::{debug, info, warn};
use regorus::CompiledPolicy;
use reqwest::Url;
use spiffe::{JwtSvid, WorkloadApiClient};
use up_rust::UAttributes;
use up_rust::communication::{
    InMemoryRpcServer, RequestHandler, RpcServer, ServiceInvocationError, UPayload,
};

use crate::cli::Cli;

mod cli;

async fn get_rpc_server(cli: cli::Cli) -> Result<InMemoryRpcServer, Box<dyn std::error::Error>> {
    let uri_provider = cli.get_local_uri_provider()?;
    let transport = cli.get_transport().await?;
    Ok(InMemoryRpcServer::new(transport, uri_provider))
}

// { "data": { "Mode": "Performance" } }
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Root {
    data: ModeMessage,
}

// Safety net so that a lock is not held forever if releasing it fails.
const SOVD_LOCK_EXPIRATION_SECS: u64 = 10;

#[derive(Debug, serde::Deserialize)]
struct SovdLock {
    id: String,
}

struct CurrentModeController {
    http_client: reqwest::Client,
    powertrain_sovd_url: Url,
    powertrain_lock_url: Url,
    workload_api: WorkloadApiClient,
    authorization_policy: CompiledPolicy,
}

impl CurrentModeController {
    async fn new(cli: &Cli) -> Result<Self, Box<dyn std::error::Error>> {
        let mut http_client_builder = reqwest::Client::builder();
        if let Some(socket_path) = cli.get_sovd_server_unix_socket() {
            http_client_builder = http_client_builder.unix_socket(socket_path.as_path());
        }
        Ok(Self {
            http_client: http_client_builder.build()?,
            powertrain_sovd_url: cli.get_sovd_powertrain_mode_resource_url()?,
            powertrain_lock_url: cli.get_sovd_powertrain_lock_resource_url()?,
            workload_api: WorkloadApiClient::connect_env().await?,
            authorization_policy: cli.opa_config.get_compiled_auth_policy()?,
        })
    }

    async fn authorize(
        &self,
        attributes: &UAttributes,
        method_id: u16,
    ) -> Result<JwtSvid, ServiceInvocationError> {
        let token = attributes.token().ok_or_else(|| {
            debug!("No token found in request attributes");
            ServiceInvocationError::Unauthenticated
        })?;
        let svid = self
            .workload_api
            .validate_jwt_token(AUDIENCE_POWERTRAIN_MODE_CONTROL, token)
            .await
            .map_err(|e| {
                debug!("Failed to validate JWT: {e}");
                ServiceInvocationError::Unauthenticated
            })?;
        debug!(
            "Successfully validated JWT provided by client [source: {}]",
            attributes.source_unchecked().to_uri(true)
        );

        let allowed = regorus::Value::from_yaml_str(&format!(
            r#"
            spiffe_id: "{}"
            method_id: {}
            "#,
            svid.spiffe_id(),
            method_id
        ))
        .and_then(|value| self.authorization_policy.eval_with_input(value))
        .map_err(|e| {
            debug!("Failed to authorize request using OPA: {e}");
            ServiceInvocationError::Internal(String::from("authorization error"))
        })?;
        if allowed.eq(&regorus::Value::Bool(false)) {
            debug!(
                "Authorization failed for SPIFFE ID {} and method ID {}",
                svid.spiffe_id(),
                method_id
            );
            Err(ServiceInvocationError::PermissionDenied(String::from(
                "not authorized to invoke method",
            )))
        } else {
            debug!(
                "Authorization successful for SPIFFE ID {} and method ID {}",
                svid.spiffe_id(),
                method_id
            );
            Ok(svid)
        }
    }

    async fn fresh_svid(&self) -> Result<JwtSvid, ServiceInvocationError> {
        self.workload_api
            .fetch_jwt_svid(&[AUDIENCE_SOVD_CDA], None)
            .await
            .map_err(|e| {
                warn!("Error retrieving JWT SVID for accessing SOVD server: {e}");
                ServiceInvocationError::Unauthenticated
            })
    }

    async fn acquire_lock(&self, svid: &JwtSvid) -> Result<String, ServiceInvocationError> {
        let response = self
            .http_client
            .post(self.powertrain_lock_url.clone())
            .bearer_auth(svid.token())
            .json(&serde_json::json!({ "lock_expiration": SOVD_LOCK_EXPIRATION_SECS }))
            .send()
            .await
            .map_err(|e| {
                warn!("Error communicating with SOVD server: {e}");
                ServiceInvocationError::Unavailable("Cannot lock powertrain ECU".to_string())
            })?;
        if !response.status().is_success() {
            warn!(
                "Failed to lock powertrain ECU, SOVD server responded with status code {}",
                response.status()
            );
            return Err(ServiceInvocationError::Unavailable(
                "Cannot lock powertrain ECU".to_string(),
            ));
        }
        response
            .json::<SovdLock>()
            .await
            .map(|lock| {
                debug!("Acquired SOVD lock on powertrain ECU [id: {}]", lock.id);
                lock.id
            })
            .map_err(|e| {
                warn!("Error parsing lock response from SOVD server: {e}");
                ServiceInvocationError::Internal("internal error".to_string())
            })
    }

    async fn release_lock(&self, svid: &JwtSvid, lock_id: &str) {
        let mut lock_url = self.powertrain_lock_url.clone();
        if lock_url
            .path_segments_mut()
            .map(|mut segments| {
                segments.push(lock_id);
            })
            .is_err()
        {
            warn!("Cannot create URL for releasing SOVD lock [id: {lock_id}]");
            return;
        }
        match self
            .http_client
            .delete(lock_url)
            .bearer_auth(svid.token())
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                debug!("Released SOVD lock on powertrain ECU [id: {lock_id}]");
            }
            Ok(response) => warn!(
                "Failed to release SOVD lock [id: {lock_id}], SOVD server responded with status code {}",
                response.status()
            ),
            Err(e) => warn!("Error releasing SOVD lock [id: {lock_id}]: {e}"),
        }
    }

    async fn get_current_mode(&self) -> Result<Option<UPayload>, ServiceInvocationError> {
        let svid = self.fresh_svid().await?;
        let response = self
            .http_client
            .get(self.powertrain_sovd_url.clone())
            .bearer_auth(svid.token())
            .send()
            .await
            .map_err(|e| {
                warn!("Error communicating with SOVD server: {e}");
                ServiceInvocationError::Unavailable("Cannot get current mode".to_string())
            })?;
        if !response.status().is_success() {
            Err(ServiceInvocationError::Unavailable(
                "Cannot get current mode".to_string(),
            ))
        } else {
            response
                .json::<Root>()
                .await
                .map(|root| root.data)
                .map_err(|e| {
                    warn!("Error parsing response from SOVD server: {e}");
                    ServiceInvocationError::Internal("internal error".to_string())
                })
                .and_then(|mode_message| {
                    serde_json::to_vec(&mode_message)
                        .map(|d| {
                            Some(UPayload::new(
                                d,
                                up_rust::UPayloadFormat::UPAYLOAD_FORMAT_JSON,
                            ))
                        })
                        .map_err(|e| {
                            warn!("Failed to serialize current mode response payload: {e}");
                            ServiceInvocationError::Internal(
                                "failed to serialize response".to_string(),
                            )
                        })
                })
        }
    }

    async fn set_current_mode(
        &self,
        request_payload: Option<UPayload>,
    ) -> Result<Option<UPayload>, ServiceInvocationError> {
        let svid = self.fresh_svid().await?;
        let mode_message = serde_json::from_slice::<ModeMessage>(
            &request_payload
                .ok_or_else(|| {
                    ServiceInvocationError::InvalidArgument("Request has no payload".to_string())
                })?
                .payload(),
        )
        .map_err(|e| {
            warn!("Failed to deserialize set mode request payload: {e}");
            ServiceInvocationError::InvalidArgument("Invalid payload".to_string())
        })?;
        let request_payload = Root {
            data: mode_message.clone(),
        };
        let lock_id = self.acquire_lock(&svid).await?;
        let result = self
            .http_client
            .put(self.powertrain_sovd_url.clone())
            .bearer_auth(svid.token())
            .json(&request_payload)
            .send()
            .await;
        self.release_lock(&svid, &lock_id).await;
        let response = result.map_err(|e| {
            warn!("Error communicating with SOVD server: {e}");
            ServiceInvocationError::Unavailable("Cannot set current mode".to_string())
        })?;
        if !response.status().is_success() {
            warn!(
                "Failed to set powertrain mode, SOVD server responded with status code {}",
                response.status()
            );
            Err(ServiceInvocationError::Unavailable(
                "Cannot set current mode".to_string(),
            ))
        } else {
            info!("Powertrain mode set to: {:?}", mode_message.mode);
            Ok(None)
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for CurrentModeController {
    async fn handle_request(
        &self,
        resource_id: u16,
        message_attributes: &UAttributes,
        request_payload: Option<UPayload>,
    ) -> Result<Option<UPayload>, ServiceInvocationError> {
        let _svid = self.authorize(message_attributes, resource_id).await?;
        match resource_id {
            common::powertrain::RESOURCE_ID_GET_CURRENT_MODE => self.get_current_mode().await,
            common::powertrain::RESOURCE_ID_SET_CURRENT_MODE => {
                self.set_current_mode(request_payload).await
            }
            _ => Err(ServiceInvocationError::Unimplemented(
                "no such operation".to_string(),
            )),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let cli = cli::Cli::parse();
    let sovd_server_uri = cli.get_sovd_server_uri().to_owned();
    let sovd_server_unix_socket = cli
        .get_sovd_server_unix_socket()
        .map_or_else(|| String::from("none"), |p| p.display().to_string());
    let mode_controller = CurrentModeController::new(&cli).await.map(Arc::new)?;

    let rpc_server = get_rpc_server(cli).await?;
    rpc_server
        .register_endpoint(
            None,
            common::powertrain::RESOURCE_ID_GET_CURRENT_MODE,
            mode_controller.clone(),
        )
        .await?;
    rpc_server
        .register_endpoint(
            None,
            common::powertrain::RESOURCE_ID_SET_CURRENT_MODE,
            mode_controller,
        )
        .await?;
    info!(
        "Powertrain mode control service is running [SOVD CDA Server: {}, Unix socket: {}]",
        sovd_server_uri, sovd_server_unix_socket
    );
    tokio::signal::ctrl_c().await?;
    info!("Powertrain mode control service is shutting down");
    Ok(())
}
