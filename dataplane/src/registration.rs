//! Control-plane self-registration (see `../../ARCHITECTURE.md`, "What
//! this MVP does not do" -> "No control-plane registration").
//!
//! **Status: RED phase of a two-phase TDD change. Nothing in this module
//! is implemented yet** — [`register_with_control_plane`] is a signature
//! only, backed by `unimplemented!()`. It exists so
//! `../tests/control_plane_registration.rs` can already specify, and
//! fail against, the exact wire contract the next phase must implement.
//! Do not call this from `main`/`build` yet.
//!
//! ## The wire contract this specifies
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

use serde::Serialize;

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

/// Registers this data plane with a control plane by issuing the PUT
/// described above against `control_plane_base_url`.
///
/// **Not implemented yet.** This is deliberately `unimplemented!()` for
/// now — see this module's doc comment and `ARCHITECTURE.md`, "No
/// control-plane registration". The next phase gives this a real body
/// (an HTTP client issuing the PUT and checking the response status) that
/// makes `../tests/control_plane_registration.rs` pass without weakening
/// that test.
pub async fn register_with_control_plane(
    control_plane_base_url: &str,
    participant_context_id: &str,
    registration: &DataPlaneRegistration,
) -> anyhow::Result<()> {
    let _ = (control_plane_base_url, participant_context_id, registration);
    unimplemented!(
        "control-plane self-registration (PUT /v5beta/participants/{{participantContextId}}/dataplanes) is not implemented yet — this is the RED step of TDD; see ARCHITECTURE.md, \"No control-plane registration\""
    )
}
