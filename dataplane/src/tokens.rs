use std::collections::HashMap;
use std::sync::RwLock;

/// One issued access token, mapping it back to the flow and dataset it
/// was minted for. Enforcement itself does not live here — see
/// `public::get_file`, which re-runs the ODRL policy on every request
/// this token authorizes (the "per GET request" enforcement-timing
/// decision — see `../ARCHITECTURE.md`).
#[derive(Clone)]
pub struct TokenRecord {
    pub flow_id: String,
    pub dataset_id: String,
}

/// In-memory bearer-token store. MVP scope (one process, one file offer)
/// does not warrant durable storage — the DPS lifecycle state this backs
/// is itself only ever held in `dataplane-sdk`'s in-memory `DataFlowRepo`.
#[derive(Default)]
pub struct TokenStore {
    tokens: RwLock<HashMap<String, TokenRecord>>,
}

impl TokenStore {
    pub fn issue(&self, flow_id: &str, dataset_id: &str) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        self.tokens.write().expect("token store lock poisoned").insert(
            token.clone(),
            TokenRecord {
                flow_id: flow_id.to_string(),
                dataset_id: dataset_id.to_string(),
            },
        );
        token
    }

    pub fn lookup(&self, token: &str) -> Option<TokenRecord> {
        self.tokens
            .read()
            .expect("token store lock poisoned")
            .get(token)
            .cloned()
    }

    /// Revokes every token issued for `flow_id` — called on TERMINATE.
    pub fn revoke_flow(&self, flow_id: &str) {
        self.tokens
            .write()
            .expect("token store lock poisoned")
            .retain(|_, record| record.flow_id != flow_id);
    }
}
