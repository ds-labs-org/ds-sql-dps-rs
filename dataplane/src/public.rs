use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_extra::headers::authorization::Bearer;
use axum_extra::headers::Authorization;
use axum_extra::TypedHeader;
use chrono::Utc;
use config_graph::ConfigGraph;
use engine::{evaluate_request, Behaviour, ClaimValue, Claims, DutyMode, WireDecision};

use crate::tokens::TokenStore;

#[derive(Clone)]
pub struct PublicState {
    pub graph: Arc<ConfigGraph>,
    pub tokens: Arc<TokenStore>,
}

/// `GET /public/{dataset_id}` — the data address every issued token
/// points at. Re-runs the ODRL policy on *every* request (not just at
/// START) so a time-bound or claim-bound constraint is enforced for the
/// life of the token, not only at the moment it was minted — see
/// "Enforcement timing" in `../ARCHITECTURE.md`.
pub async fn get_file(
    Path(dataset_id): Path<String>,
    State(state): State<PublicState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Response {
    let Some(record) = state.tokens.lookup(auth.token()) else {
        return (StatusCode::UNAUTHORIZED, "invalid or expired token").into_response();
    };
    if record.dataset_id != dataset_id {
        return (StatusCode::FORBIDDEN, "token not valid for this dataset").into_response();
    }

    let policy_doc = match state.graph.policy_jsonld(&dataset_id) {
        Ok(doc) => doc,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let ingested = match dsp_odrl_adapter::ingest_policy_value(&policy_doc) {
        Ok(ingested) => ingested,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("policy ingestion failed: {e}"),
            )
                .into_response()
        }
    };

    let mut claims = Claims::new();
    claims.insert("dateTime".to_string(), ClaimValue::from(Utc::now().to_rfc3339()));

    let asset_iri = config_graph::dataset_iri(&dataset_id);
    let request = dsp_odrl_adapter::request_for(
        &ingested.policy,
        &asset_iri,
        "use",
        claims,
        DutyMode::Advise,
        Behaviour::Closed,
    );
    let response = evaluate_request(&request);

    if response.decision != WireDecision::Allow {
        return (
            StatusCode::FORBIDDEN,
            format!("policy denied this request: {}", response.reason),
        )
            .into_response();
    }

    let file_path = match state.graph.file_path(&dataset_id) {
        Ok(path) => path,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let media_type = state
        .graph
        .media_type(&dataset_id)
        .unwrap_or_else(|_| "application/octet-stream".to_string());

    match tokio::fs::read(&file_path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, media_type)], bytes).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read {}: {e}", file_path.display()),
        )
            .into_response(),
    }
}
