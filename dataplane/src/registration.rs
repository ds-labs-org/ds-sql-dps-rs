//! Control-plane self-registration (see `../../ARCHITECTURE.md`, "What
//! this MVP does not do" and "What has actually been run").
//!
//! ## The wire contract this implements
//!
//! Ground truth read directly from the vendored EDC connector source
//! (`vendor/eclipse-edc-connector/data-protocols/data-plane-signaling/
//! data-plane-signaling-core/src/main/java/org/eclipse/edc/signaling/`,
//! in the parent `dataspace` repo — not this submodule):
//!
//! - `port/api/management/v5/DataPlaneRegistrationApiV5Controller.java` +
//!   `.../v5/DataPlaneRegistrationApiV5.java`: a data plane registers
//!   itself with `PUT /v5beta/participants/{participantContextId}/dataplanes`.
//! - `domain/DataPlaneRegistrationMessage.java`: the request body is a
//!   JSON object `{ dataplaneId, endpoint, transferTypes, labels,
//!   authorization }` — [`DataPlaneRegistration`] mirrors those fields.
//!
//! This v5beta endpoint lives in the same data-plane-signaling module
//! family that `dataplane-sdk`/`dataplane-sdk-axum` (this project's own
//! signaling dependency) implements the wire protocol for; it supersedes
//! the older, separate `DataPlaneSelectorApiV3`/`V4` also present in that
//! vendored source.
//!
//! ## Where this is called from, and how failure is treated
//!
//! [`register_with_control_plane`] is called at most once, at startup,
//! from `crate::build` — and only when `CONTROL_PLANE_URL` is set (see
//! `crate::config::Config::control_plane_url`). It is deliberately
//! **optional and non-fatal**: a failure (the control plane unreachable,
//! or answering with a non-2xx status) is logged as a `tracing::warn!`
//! and the data plane keeps starting, so the existing
//! demo-without-a-control-plane workflow this project has always
//! supported (see `../../ARCHITECTURE.md`, "What has actually been run")
//! keeps working unchanged whether or not a control plane is configured
//! or reachable. There is no retry or periodic re-registration — see
//! `../../ARCHITECTURE.md`'s "What this MVP does not do" for that
//! remaining gap.

use serde::Serialize;
use thiserror::Error;

/// The JSON body of a `PUT /v5beta/participants/{participantContextId}/dataplanes`
/// self-registration request, matching the field names of the EDC
/// connector's own `DataPlaneRegistrationMessage` Java record (see this
/// module's doc comment for exactly where that was read).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPlaneRegistration {
    /// This data plane's own self-chosen, stable identifier.
    pub dataplane_id: String,
    /// This data plane's own signaling base URL — where the control
    /// plane should send `DataFlowStartMessage`/`.../terminate` etc (the
    /// same base `dataplane-sdk-axum::router::router()` is mounted on),
    /// *not* the `/public/{dataset_id}` file-serving endpoint.
    pub endpoint: String,
    /// Transfer types this data plane can serve, e.g. `"HttpData-PULL"`.
    pub transfer_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<serde_json::Value>,
}

/// Everything that can go wrong issuing the self-registration PUT,
/// distinguishing a request that never made it to the control plane from
/// one the control plane actively rejected.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// The PUT itself failed to send (DNS failure, connection refused,
    /// TLS error, timeout, ...) — the control plane never got a chance to
    /// respond.
    #[error("failed to send data-plane registration request to {url}: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    /// The control plane received the request and answered, but with a
    /// non-2xx status.
    #[error("control plane at {url} rejected data-plane registration with status {status}: {body}")]
    NonSuccessStatus {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
}

/// Registers this data plane with a control plane by issuing the
/// `PUT /v5beta/participants/{participant_context_id}/dataplanes` request
/// described in this module's doc comment against `control_plane_base_url`.
pub async fn register_with_control_plane(
    control_plane_base_url: &str,
    participant_context_id: &str,
    registration: &DataPlaneRegistration,
) -> Result<(), RegistrationError> {
    let url = format!(
        "{}/v5beta/participants/{participant_context_id}/dataplanes",
        control_plane_base_url.trim_end_matches('/')
    );

    let client = reqwest::Client::new();
    let response = client
        .put(&url)
        .json(registration)
        .send()
        .await
        .map_err(|source| RegistrationError::Request {
            url: url.clone(),
            source,
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
        return Err(RegistrationError::NonSuccessStatus { url, status, body });
    }

    Ok(())
}
