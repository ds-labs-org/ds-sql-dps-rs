//! RED-phase TDD test for control-plane self-registration (see
//! `../../ARCHITECTURE.md`, "What this MVP does not do" -> "No
//! control-plane registration").
//!
//! This test specifies the wire contract `dataplane::registration::
//! register_with_control_plane` must satisfy against a real control
//! plane's EDC Data Plane Signaling v5beta self-registration endpoint —
//! `PUT /v5beta/participants/{participantContextId}/dataplanes` — using a
//! mocked control plane (`wiremock`) rather than a real one, and asserts
//! the mock actually received exactly the request it expects (path,
//! method, and the three body fields the ground-truth Java controller
//! requires: `dataplaneId`, `endpoint`, `transferTypes`).
//!
//! `register_with_control_plane` is currently `unimplemented!()` (see its
//! doc comment in `../src/registration.rs`) — it exists purely as the
//! signature this test drives. Running this test is expected to FAIL
//! (panic on the `unimplemented!()` call) until the next phase gives it a
//! real body. Do not "fix" this test by weakening its assertions; the
//! next phase must make `register_with_control_plane` actually issue the
//! PUT this test verifies.

use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use dataplane::registration::{DataPlaneRegistration, register_with_control_plane};

/// Fixed test participant-context id — this data plane's registration
/// request path is scoped to it (`/v5beta/participants/{id}/dataplanes`).
const PARTICIPANT_CONTEXT_ID: &str = "test-participant-context";

/// Fixed test id for this data plane's own registration.
const DATAPLANE_ID: &str = "ds-sql-dps-rs-test-dataplane";

/// This data plane's own signaling base URL, as it would advertise it to
/// a real control plane (see `DataPlaneRegistration::endpoint`'s doc
/// comment for why this is the signaling base, not the public/file base).
const SIGNALING_BASE_URL: &str = "http://localhost:9191";

#[tokio::test]
async fn registers_this_dataplane_with_the_control_plane_on_startup() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path(format!(
            "/v5beta/participants/{PARTICIPANT_CONTEXT_ID}/dataplanes"
        )))
        .and(body_partial_json(serde_json::json!({
            "dataplaneId": DATAPLANE_ID,
            "endpoint": SIGNALING_BASE_URL,
            "transferTypes": ["HttpData-PULL"],
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .named("PUT .../dataplanes self-registration")
        .mount(&mock_server)
        .await;

    let registration = DataPlaneRegistration {
        dataplane_id: DATAPLANE_ID.to_string(),
        endpoint: SIGNALING_BASE_URL.to_string(),
        transfer_types: vec!["HttpData-PULL".to_string()],
        labels: None,
        authorization: None,
    };

    register_with_control_plane(&mock_server.uri(), PARTICIPANT_CONTEXT_ID, &registration)
        .await
        .expect("registration PUT against the mocked control plane should succeed");

    // Belt-and-braces: even if `register_with_control_plane` returned Ok
    // without actually calling out (which `.expect(1)` above would also
    // catch at `mock_server` shutdown), fail loudly and immediately here.
    mock_server.verify().await;
}
