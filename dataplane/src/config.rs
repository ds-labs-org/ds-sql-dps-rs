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
    /// This data plane's own participant-context id, reused both as the
    /// `dataplane-sdk` `ParticipantContext.id` (`crate::build`) and as the
    /// `{participantContextId}` path segment of the control-plane
    /// self-registration PUT (`crate::registration`) — one identifier,
    /// not two independently-invented ones.
    pub participant_context_id: String,
    /// This data plane's own self-chosen, stable identifier, advertised
    /// to the control plane as `DataPlaneRegistration.dataplane_id`.
    pub dataplane_id: String,
    /// Base URL of an EDC control plane's Data Plane Signaling v5beta API
    /// to self-register with at startup (`PUT
    /// /v5beta/participants/{participant_context_id}/dataplanes`) — see
    /// `crate::registration`. `None` (the default) skips registration
    /// entirely; this is how the demo-without-a-control-plane workflow
    /// keeps working. When set, a failed registration attempt is logged
    /// as a warning and does not stop the data plane from starting.
    pub control_plane_url: Option<String>,
    /// Conformance-testing escape hatch for the official Data Plane
    /// Signaling TCK (see `dataplane/tests/dps_tck.rs` and
    /// `../ARCHITECTURE.md`, "Data Plane Signaling TCK conformance").
    /// `false` in every normal run (the default): `FileOfferHandler::on_start`
    /// rejects any `dataset_id` the configuration graph wasn't explicitly
    /// seeded with, per this MVP's one-file-offer scope. `true` (set via
    /// `TCK_MODE=1`/`true`) instead auto-seeds a fresh offer — cloned from
    /// this same `Config`'s own [`Config::file_offer`], just with
    /// `dataset_id` overridden — the first time a request names a dataset
    /// id the graph doesn't already know, because the official TCK mints a
    /// fresh random UUID `datasetId` per test run that this data plane has
    /// no way to know in advance and no legitimate reason to accept
    /// outside of conformance testing.
    pub tck_mode: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> Self {
        let public_port: u16 = env_or("PUBLIC_PORT", "9192")
            .parse()
            .expect("PUBLIC_PORT must be a u16");
        let public_base_url = std::env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| format!("http://localhost:{public_port}"));

        Self {
            dataset_id: env_or("DATASET_ID", "primary"),
            file_path: PathBuf::from(env_or("FILE_PATH", "sample-data/sample.csv")),
            media_type: env_or("MEDIA_TYPE", "text/csv"),
            assigner: env_or("ASSIGNER", "ds-sql-dps-rs"),
            policy_not_after: std::env::var("POLICY_NOT_AFTER").ok(),
            signaling_port: env_or("SIGNALING_PORT", "9191")
                .parse()
                .expect("SIGNALING_PORT must be a u16"),
            public_port,
            public_base_url,
            participant_context_id: env_or("PARTICIPANT_CONTEXT_ID", "ds-sql-dps-rs"),
            dataplane_id: env_or("DATAPLANE_ID", "ds-sql-dps-rs-dataplane"),
            control_plane_url: std::env::var("CONTROL_PLANE_URL").ok(),
            tck_mode: std::env::var("TCK_MODE")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
        }
    }

    /// Test-oriented constructor. `signaling_port` and `public_port` are
    /// both set to `0` (bind an OS-assigned ephemeral port) so parallel
    /// tests never collide on this crate's fixed default ports; the
    /// caller's `file_path` is used as-is, and `dataset_id`/`media_type`/
    /// `assigner` get fixed demo values. `public_base_url` is left empty
    /// here — `crate::build` derives the real one from the public
    /// listener's actually-assigned port whenever `public_port == 0` (see
    /// its doc comment), which is the whole reason tests use this
    /// constructor instead of `from_env`.
    ///
    /// `file_path` is a required parameter rather than defaulting to
    /// `"sample-data/sample.csv"` like [`Config::from_env`] does, because
    /// `cargo test` runs with this crate's own directory (`dataplane/`)
    /// as its working directory, not the workspace root — that relative
    /// path would not resolve. Callers should anchor their own path off
    /// `env!("CARGO_MANIFEST_DIR")` (which *is* `dataplane/`), e.g.
    /// `PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../sample-data/sample.csv"))`,
    /// or write a temp file of their own and pass that.
    pub fn for_test(file_path: PathBuf) -> Self {
        Self {
            dataset_id: "primary".to_string(),
            file_path,
            media_type: "text/csv".to_string(),
            assigner: "ds-sql-dps-rs-test".to_string(),
            policy_not_after: None,
            signaling_port: 0,
            public_port: 0,
            public_base_url: String::new(),
            participant_context_id: "ds-sql-dps-rs".to_string(),
            dataplane_id: "ds-sql-dps-rs-dataplane".to_string(),
            control_plane_url: None,
            tck_mode: false,
        }
    }

    /// Builder-style override for [`Config::for_test`]'s `policy_not_after`
    /// (left `None` by `for_test` itself), so a test can seed the same
    /// `dateTime lteq ...` constraint `POLICY_NOT_AFTER` sets in
    /// production, without needing a second constructor — see
    /// `crate::public::get_file`'s per-request re-evaluation, which is
    /// what this is for exercising.
    pub fn with_policy_not_after(mut self, not_after: impl Into<String>) -> Self {
        self.policy_not_after = Some(not_after.into());
        self
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
