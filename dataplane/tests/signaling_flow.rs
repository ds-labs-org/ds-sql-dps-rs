//! End-to-end integration tests for the MVP data plane, turning the
//! manually-verified scenario list in `../../ARCHITECTURE.md`'s "What has
//! actually been run" into automated coverage: real HTTP calls (via
//! `reqwest`) against a whole app instance (`dataplane::build`) booted
//! in-process on OS-assigned ephemeral ports (`Config::for_test`), never
//! against a shared/running server.
//!
//! Every test builds and boots its own [`Running`] app — its own
//! configuration graph, token store, and pair of listeners — so tests can
//! run concurrently (the default `cargo test` behavior) without colliding
//! on shared state or a fixed port. None of these tests need `#[serial]`:
//! there is no cross-test shared state to race on.

use std::path::PathBuf;

use dataplane::config::Config;
use dataplane_sdk::core::model::messages::{
    DataFlowPrepareMessage, DataFlowStartMessage, DataFlowStatusMessage, DataFlowSuspendMessage,
    DataFlowTerminateMessage,
};
use reqwest::StatusCode;

/// The dataset id every `Config::for_test`-built app seeds (see
/// `Config::for_test`'s fixed `dataset_id`).
const DATASET_ID: &str = "primary";

/// The bundled demo file, addressed the way `Config::for_test`'s own doc
/// comment recommends (`cargo test`'s working directory is `dataplane/`,
/// not the workspace root) — this also means every test exercises the
/// exact same known bytes/media type without a `tempfile` dependency.
fn sample_file() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sample-data/sample.csv"
    ))
}

/// A booted app instance: both listeners already accepting connections on
/// their OS-assigned ports (`dataplane::serve` running on a spawned task
/// for the lifetime of the owning test's tokio runtime).
struct Running {
    signaling_base: String,
    public_base: String,
}

impl Running {
    /// Builds and boots a whole app from `config` (see `dataplane::build`),
    /// then hands its `serve` loop to a spawned task and returns the two
    /// base URLs a test needs. The task is never joined: it runs until the
    /// test's own (per-`#[tokio::test]`) runtime is torn down at the end of
    /// the test, which drops it.
    async fn spawn(config: Config) -> Self {
        let app = dataplane::build(config).await.expect("failed to build app");
        let signaling_addr = app
            .signaling_listener
            .local_addr()
            .expect("signaling listener has a local addr");
        let public_addr = app
            .public_listener
            .local_addr()
            .expect("public listener has a local addr");

        tokio::spawn(async move {
            let _ = dataplane::serve(app).await;
        });

        // `dataplane::build` derives the public base URL it puts in every
        // issued token's `DataAddress.endpoint` as `http://localhost:<port>`
        // (see its doc comment) whenever `public_port == 0`, not from the
        // listener's own `local_addr()` (which reports the bind address,
        // `0.0.0.0`) — matching that here is what lets the assertions below
        // compare the endpoint by exact string rather than just a suffix.
        Self {
            signaling_base: format!("http://localhost:{}", signaling_addr.port()),
            public_base: format!("http://localhost:{}", public_addr.port()),
        }
    }

    fn start_url(&self) -> String {
        format!("{}/api/v1/dataflows/start", self.signaling_base)
    }

    fn prepare_url(&self) -> String {
        format!("{}/api/v1/dataflows/prepare", self.signaling_base)
    }

    fn terminate_url(&self, data_flow_id: &str) -> String {
        format!(
            "{}/api/v1/dataflows/{data_flow_id}/terminate",
            self.signaling_base
        )
    }

    fn suspend_url(&self, data_flow_id: &str) -> String {
        format!(
            "{}/api/v1/dataflows/{data_flow_id}/suspend",
            self.signaling_base
        )
    }

    fn public_url(&self, dataset_id: &str) -> String {
        format!("{}/public/{dataset_id}", self.public_base)
    }
}

fn start_message(dataset_id: &str, data_flow_id: &str) -> DataFlowStartMessage {
    DataFlowStartMessage::builder()
        .message_id(uuid::Uuid::new_v4().to_string())
        .participant_id("test-consumer")
        .counter_party_id("test-provider")
        .dataspace_context("dsp")
        .data_flow_id(data_flow_id)
        .agreement_id("agreement-1")
        .dataset_id(dataset_id)
        .profile("dsp-http-pull")
        .build()
}

fn prepare_message(dataset_id: &str, data_flow_id: &str) -> DataFlowPrepareMessage {
    DataFlowPrepareMessage::builder()
        .message_id(uuid::Uuid::new_v4().to_string())
        .participant_id("test-consumer")
        .counter_party_id("test-provider")
        .dataspace_context("dsp")
        .data_flow_id(data_flow_id)
        .agreement_id("agreement-1")
        .dataset_id(dataset_id)
        .profile("dsp-http-pull")
        .build()
}

/// Issues a fresh flow id and calls START for `dataset_id`, returning the
/// parsed `DataFlowStatusMessage` on success — the common setup step for
/// every test that needs a live token.
async fn start(
    client: &reqwest::Client,
    running: &Running,
    dataset_id: &str,
) -> (String, DataFlowStatusMessage) {
    let data_flow_id = uuid::Uuid::new_v4().to_string();
    let response = client
        .post(running.start_url())
        .json(&start_message(dataset_id, &data_flow_id))
        .send()
        .await
        .expect("START request failed to send");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "START with a known dataset id should succeed"
    );
    let status: DataFlowStatusMessage = response.json().await.expect("START response is JSON");
    (data_flow_id, status)
}

/// 1. START with a valid, known dataset id returns 200 with a
///    `DataAddress` carrying an `"authorization"` endpoint property (a
///    bearer token) and an endpoint pointing at `/public/{dataset_id}`.
#[tokio::test]
async fn start_returns_data_address_with_token_and_public_endpoint() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let (_data_flow_id, status) = start(&client, &running, DATASET_ID).await;

    let data_address = status
        .data_address
        .expect("START response carries a DataAddress");
    assert_eq!(data_address.endpoint, running.public_url(DATASET_ID));

    let token = data_address
        .get_property("authorization")
        .expect("DataAddress carries an 'authorization' endpoint property");
    assert!(
        !token.is_empty(),
        "the authorization token must not be empty"
    );
    assert_eq!(data_address.get_property("authType"), Some("bearer"));
}

/// 2. GET the public endpoint with that token succeeds (200) and streams
///    the exact bytes of the configured sample file with the configured
///    media type as `Content-Type`.
#[tokio::test]
async fn get_public_file_with_valid_token_returns_configured_bytes() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let (_data_flow_id, status) = start(&client, &running, DATASET_ID).await;
    let token = status
        .data_address
        .expect("DataAddress")
        .get_property("authorization")
        .expect("authorization property")
        .to_string();

    let response = client
        .get(running.public_url(DATASET_ID))
        .bearer_auth(&token)
        .send()
        .await
        .expect("GET request failed to send");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .expect("Content-Type header is present")
            .to_str()
            .expect("Content-Type is valid UTF-8"),
        "text/csv",
    );

    let body = response.bytes().await.expect("response body");
    let expected =
        std::fs::read(sample_file()).expect("sample-data/sample.csv exists and is readable");
    assert_eq!(body.as_ref(), expected.as_slice());
}

/// 3. GET the public endpoint with no `Authorization` header at all is
///    rejected. This is *not* the 401 the token-lookup path returns (see
///    the next test) — `axum_extra::TypedHeader<Authorization<Bearer>>`'s
///    own rejection (`TypedHeaderRejection::into_response`, read from the
///    vendored `axum-extra` source) fires before `public::get_file`'s body
///    ever runs, and always answers 400 Bad Request for a missing header.
#[tokio::test]
async fn get_public_file_without_authorization_header_is_rejected() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let response = client
        .get(running.public_url(DATASET_ID))
        .send()
        .await
        .expect("GET request failed to send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// 4. GET the public endpoint with a syntactically valid but unknown
///    bearer token is rejected with 401 (`TokenStore::lookup` returns
///    `None`).
#[tokio::test]
async fn get_public_file_with_unknown_token_is_rejected() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let response = client
        .get(running.public_url(DATASET_ID))
        .bearer_auth("garbage-unknown-token")
        .send()
        .await
        .expect("GET request failed to send");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 5. GET the public endpoint with a valid token but the wrong dataset id
///    in the path is rejected with 403 (the token's recorded dataset id
///    doesn't match the path).
#[tokio::test]
async fn get_public_file_with_wrong_dataset_id_is_rejected() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let (_data_flow_id, status) = start(&client, &running, DATASET_ID).await;
    let token = status
        .data_address
        .expect("DataAddress")
        .get_property("authorization")
        .expect("authorization property")
        .to_string();

    let response = client
        .get(running.public_url("not-the-seeded-dataset"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("GET request failed to send");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// 6. START with an unknown dataset id (one the configuration graph was
///    never seeded with) fails. `FileOfferHandler::on_start` maps this to
///    `HandlerError::NotSupported`, which `dataplane-sdk`'s own `SdkError`
///    wraps as `SdkError::Handler(_)`; `dataplane-sdk-axum`'s
///    `SignalingError::into_response` (read from the vendored source) has
///    no arm for `SdkError::Handler` specifically, so it falls through to
///    the catch-all `SignalingError::Sdk(e)` arm: 500 Internal Server
///    Error, not a 4xx.
#[tokio::test]
async fn start_with_unknown_dataset_id_fails() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let response = client
        .post(running.start_url())
        .json(&start_message(
            "no-such-dataset",
            &uuid::Uuid::new_v4().to_string(),
        ))
        .send()
        .await
        .expect("START request failed to send");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// 7. TERMINATE revokes the token: a GET that succeeded before TERMINATE
///    with the same token fails (401) afterward.
#[tokio::test]
async fn terminate_revokes_the_token() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let (data_flow_id, status) = start(&client, &running, DATASET_ID).await;
    let token = status
        .data_address
        .expect("DataAddress")
        .get_property("authorization")
        .expect("authorization property")
        .to_string();

    let before = client
        .get(running.public_url(DATASET_ID))
        .bearer_auth(&token)
        .send()
        .await
        .expect("GET request failed to send");
    assert_eq!(
        before.status(),
        StatusCode::OK,
        "token should work pre-terminate"
    );

    let terminate = client
        .post(running.terminate_url(&data_flow_id))
        .json(&DataFlowTerminateMessage {
            reason: Some("integration test done".to_string()),
        })
        .send()
        .await
        .expect("TERMINATE request failed to send");
    assert_eq!(terminate.status(), StatusCode::OK);

    let after = client
        .get(running.public_url(DATASET_ID))
        .bearer_auth(&token)
        .send()
        .await
        .expect("GET request failed to send");
    assert_eq!(
        after.status(),
        StatusCode::UNAUTHORIZED,
        "the same token must be rejected once its flow is terminated"
    );
}

/// 8. Per-request policy enforcement: a policy whose `dateTime lteq ...`
///    constraint is already in the past still lets START succeed (the MVP
///    mints a token unconditionally — see "Enforcement timing" in
///    `../../ARCHITECTURE.md`), but the following GET is denied (403) by a
///    live `engine::evaluate_request` call, with a reason that mentions
///    the policy.
#[tokio::test]
async fn expired_policy_denies_get_but_not_start() {
    let config = Config::for_test(sample_file()).with_policy_not_after("2000-01-01T00:00:00Z");
    let running = Running::spawn(config).await;
    let client = reqwest::Client::new();

    let (_data_flow_id, status) = start(&client, &running, DATASET_ID).await;
    let token = status
        .data_address
        .expect("START must still succeed and carry a DataAddress")
        .get_property("authorization")
        .expect("authorization property")
        .to_string();

    let response = client
        .get(running.public_url(DATASET_ID))
        .bearer_auth(&token)
        .send()
        .await
        .expect("GET request failed to send");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response.text().await.expect("response body is text");
    assert!(
        body.to_lowercase().contains("policy"),
        "denial reason should mention the policy, got: {body}"
    );
}

/// 9. PREPARE and SUSPEND are both signaling calls this MVP data plane
///    deliberately does not support (see "Signaling scope" in
///    `../../ARCHITECTURE.md`): both must answer with an error response,
///    not a 200. Per the same `SdkError::Handler(HandlerError::NotSupported)`
///    -> catch-all mapping read for the START case above, that error
///    response is 500.
#[tokio::test]
async fn prepare_and_suspend_return_error_responses() {
    let running = Running::spawn(Config::for_test(sample_file())).await;
    let client = reqwest::Client::new();

    let prepare_response = client
        .post(running.prepare_url())
        .json(&prepare_message(
            DATASET_ID,
            &uuid::Uuid::new_v4().to_string(),
        ))
        .send()
        .await
        .expect("PREPARE request failed to send");
    assert_eq!(prepare_response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // SUSPEND is only reachable for a flow the repo already knows about
    // (`DataPlaneSdkInternal::suspend` fetches it before ever calling
    // `on_suspend`), so START one first.
    let (data_flow_id, _status) = start(&client, &running, DATASET_ID).await;

    let suspend_response = client
        .post(running.suspend_url(&data_flow_id))
        .json(&DataFlowSuspendMessage { reason: None })
        .send()
        .await
        .expect("SUSPEND request failed to send");
    assert_eq!(suspend_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
