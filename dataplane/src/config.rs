use std::path::PathBuf;

use config_graph::{ConstraintSpec, FileOffer, PermissionSpec};

/// Startup configuration, read from environment variables so the MVP
/// needs no config file/CLI-parsing dependency. Every variable has a
/// demo-friendly default so `cargo run` works against the bundled
/// `sample-data/sample.csv` out of the box.
pub struct Config {
    pub dataset_id: String,
    pub file_path: PathBuf,
    pub media_type: String,
    pub assigner: String,
    /// An optional `dateTime lteq <value>` constraint on the `use`
    /// permission, as an RFC 3339 timestamp — set `POLICY_NOT_AFTER` to
    /// see the per-GET enforcement (`crate::public::get_file`) actually
    /// deny a request once the deadline passes.
    pub policy_not_after: Option<String>,
    pub signaling_port: u16,
    pub public_port: u16,
    pub public_base_url: String,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> Self {
        let public_port: u16 = env_or("PUBLIC_PORT", "9192").parse().expect("PUBLIC_PORT must be a u16");
        let public_base_url = std::env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| format!("http://localhost:{public_port}"));

        Self {
            dataset_id: env_or("DATASET_ID", "primary"),
            file_path: PathBuf::from(env_or("FILE_PATH", "sample-data/sample.csv")),
            media_type: env_or("MEDIA_TYPE", "text/csv"),
            assigner: env_or("ASSIGNER", "ds-sql-dps-rs"),
            policy_not_after: std::env::var("POLICY_NOT_AFTER").ok(),
            signaling_port: env_or("SIGNALING_PORT", "9191").parse().expect("SIGNALING_PORT must be a u16"),
            public_port,
            public_base_url,
        }
    }

    /// Builds the one [`FileOffer`] this MVP seeds its configuration graph
    /// with at startup.
    pub fn file_offer(&self) -> FileOffer {
        let constraints = self
            .policy_not_after
            .iter()
            .map(|not_after| ConstraintSpec {
                left_operand: "dateTime".to_string(),
                operator: "lteq".to_string(),
                right_operand: not_after.clone(),
            })
            .collect();

        FileOffer {
            dataset_id: self.dataset_id.clone(),
            title: format!("{} (ds-sql-dps-rs data offer)", self.file_path.display()),
            file_path: self.file_path.clone(),
            media_type: self.media_type.clone(),
            assigner: self.assigner.clone(),
            permissions: vec![PermissionSpec {
                action: "use".to_string(),
                constraints,
            }],
        }
    }
}
