//! Runs the official Eclipse Dataspace TCK's Data Plane Signaling
//! conformance suite (the `eclipsedataspacetck/dps-tck-runtime` container)
//! against a real, fully-booted instance of this data plane
//! (`dataplane::build`) — not a mock, and not a reimplementation of the
//! TCK's own assertions. Modelled directly on the upstream
//! `dataplane-sdk-rust`'s own TCK harness
//! (`crates/sdk-tck-tests/tests/{tck_tests,util}.rs` at
//! <https://github.com/eclipse-dataplane-core/dataplane-sdk-rust>), which
//! this project's `dataplane-sdk`/`dataplane-sdk-axum` dependency comes
//! from.
//!
//! ## Scope: what this actually proves, and what it doesn't
//!
//! This project's data plane is PULL-only, PROVIDER-only, and implements
//! only START and TERMINATE (see `../../ARCHITECTURE.md`, "Signaling
//! scope" and "What this MVP does not do"). `dps.tck.properties` narrows
//! the TCK run to the narrowest scope its own config-driven filters can
//! reach (see that file's comments for exactly why), but that scope still
//! includes the *consumer* role's tests and the provider's SUSPEND/RESUME
//! tests — neither of which this MVP implements, and neither of which the
//! TCK's filters (test-package selection, JUnit tag include/exclude; no
//! per-method selector exists) can exclude on their own.
//!
//! So rather than either faking a green run by asserting nothing
//! meaningful, or skipping the exercise entirely, this test asserts the
//! *exact* set of TCK test ids already known to fail for those documented,
//! out-of-scope reasons ([`EXPECTED_FAILURES`]). That means:
//!
//! - a regression on either of the two tests this MVP actually claims to
//!   satisfy (`DP_P_PULL:01-01` — START + a completed notification;
//!   `DP_P_PULL:01-02` — START + TERMINATE) still fails this test;
//! - the TCK unexpectedly reporting *fewer* failures than
//!   [`EXPECTED_FAILURES`] also fails this test — a sign the list (and
//!   `../../ARCHITECTURE.md`'s scope claims) have gone stale, not a
//!   reason to celebrate a silent pass;
//! - an entirely new/different failure (e.g. `DP_P_PULL:01-01` itself
//!   starting to fail) fails this test the same way a missing expected
//!   failure does — the assertion is an exact-set comparison, not a
//!   subset check.
//!
//! ## Running this
//!
//! Requires Docker (pulls and runs
//! `eclipsedataspacetck/dps-tck-runtime:1.3.0`, ~230MB, plus the base
//! image layers). Not run by plain `cargo test` or the "quality" CI job —
//! `#[ignore]`d so neither needs Docker:
//!
//! ```sh
//! cargo test --test dps_tck -- --ignored --nocapture
//! ```
//!
//! Verified genuinely green against a real, locally running
//! `dps-tck-runtime:1.3.0` container (not written blind) — see
//! `../../ARCHITECTURE.md`, "Data Plane Signaling TCK conformance" for
//! the exact run this was checked against.

use std::collections::BTreeSet;
use std::path::{self, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use dataplane::config::Config;
use futures::FutureExt;
use futures::future::BoxFuture;
use regex::Regex;
use testcontainers::core::logs::LogFrame;
use testcontainers::core::logs::consumer::LogConsumer;
use testcontainers::core::{ContainerPort, Host, IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

/// Fixed (not OS-assigned) ports: `dps.tck.properties`'
/// `dataspacetck.dps.dataplane.url` is a static file bind-mounted into
/// the TCK container, so it can't learn a port picked at runtime the way
/// `dataplane`'s other integration tests do (see `Config::for_test`'s doc
/// comment on why *they* use ephemeral ports instead). Distinct from this
/// project's own documented defaults (9191/9192, see
/// `../../ARCHITECTURE.md`, "Running it") so this test can't collide with
/// a `cargo run` left running on the same machine.
const SIGNALING_PORT: u16 = 19191;
const PUBLIC_PORT: u16 = 19192;

/// The exact TCK test ids (`ClassTagPrefix:NN-NN`, as printed by the TCK
/// runtime's own `TckExecutionListener`) expected to fail against this
/// MVP, and why. See this file's module doc comment for what asserting an
/// exact set (rather than "no failures") buys, and `dps.tck.properties`
/// for why the TCK's own filters can't exclude these outright.
const EXPECTED_FAILURES: &[&str] = &[
    // Consumer role (`DataFlowType::Consumer`, driven by PREPARE) is not
    // implemented — `FileOfferHandler::on_prepare` (dataplane/src/handler.rs)
    // always answers `HandlerError::NotSupported`.
    "DP_C_PULL:01-01",
    "DP_C_PULL:01-02",
    "DP_C_PULL:02-01",
    "DP_C_PULL:02-02",
    "DP_C_PULL:03-01",
    // SUSPEND is not implemented — `FileOfferHandler::on_suspend`
    // (dataplane/src/handler.rs) always answers `HandlerError::NotSupported`,
    // a deliberate MVP decision (see ../../ARCHITECTURE.md, "What this
    // MVP does not do"), not a bug to fix here.
    "DP_P_PULL:02-01",
    "DP_P_PULL:02-02",
];

fn sample_file() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sample-data/sample.csv"
    ))
}

#[tokio::test]
#[ignore = "needs Docker; run explicitly with `cargo test --test dps_tck -- --ignored --nocapture`"]
async fn dps_signaling_tck_matches_documented_pull_provider_scope() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    // `TCK_MODE: true` is the whole point of this test: the TCK mints a
    // fresh random UUID `datasetId` per test run (see
    // `Config::tck_mode`'s doc comment), which this data plane would
    // otherwise reject outright at START.
    let config = Config {
        dataset_id: "tck-startup-seed".to_string(),
        file_path: sample_file(),
        media_type: "text/csv".to_string(),
        assigner: "ds-sql-dps-rs-tck-test".to_string(),
        policy_not_after: None,
        signaling_port: SIGNALING_PORT,
        public_port: PUBLIC_PORT,
        public_base_url: format!("http://localhost:{PUBLIC_PORT}"),
        participant_context_id: "ds-sql-dps-rs".to_string(),
        dataplane_id: "ds-sql-dps-rs-dataplane".to_string(),
        control_plane_url: None,
        tck_mode: true,
    };

    let app = dataplane::build(config)
        .await
        .expect("failed to build the data plane app");
    tokio::spawn(async move {
        let _ = dataplane::serve(app).await;
    });
    wait_for_port(SIGNALING_PORT).await;

    let reporter = TckReporter::default();
    let properties_path = Path::new("tests/dps.tck.properties");
    let properties_path = path::absolute(properties_path)
        .expect("dps.tck.properties path resolves to an absolute path");

    let _tck = GenericImage::new("eclipsedataspacetck/dps-tck-runtime", "1.3.0")
        .with_exposed_port(8083.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Test run complete"))
        .with_mapped_port(8083, ContainerPort::Tcp(8083))
        .with_mount(Mount::bind_mount(
            properties_path
                .to_str()
                .expect("dps.tck.properties path is valid UTF-8"),
            "/etc/tck/config.properties",
        ))
        // The TCK container calls back into this host's own data plane
        // (`dataspacetck.dps.dataplane.url` in dps.tck.properties) over
        // this address, since the data plane runs on the host, not in a
        // container of its own.
        .with_host("host.docker.internal", Host::HostGateway)
        .with_log_consumer(reporter.clone())
        .start()
        .await
        .expect("failed to start the dps-tck-runtime container");

    let actual: BTreeSet<String> = reporter.failures().into_iter().collect();
    let expected: BTreeSet<String> = EXPECTED_FAILURES.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        actual, expected,
        "TCK failure set no longer matches EXPECTED_FAILURES. Either a real \
         regression (a new/different failure appeared — check for \
         DP_P_PULL:01-01 or DP_P_PULL:01-02 first, the two this MVP claims to \
         satisfy) or this MVP now covers more than EXPECTED_FAILURES documents \
         (some entries no longer fail). Update EXPECTED_FAILURES and \
         ../../ARCHITECTURE.md together with whichever is true — never just \
         to make this test pass again. Actual failures reported by the TCK \
         this run: {actual:?}"
    );
}

/// Polls the signaling API's own port until it accepts a TCP connection,
/// so the TCK container is never started against a data plane that
/// hasn't finished binding yet.
async fn wait_for_port(port: u16) {
    let addr = format!("127.0.0.1:{port}");
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("signaling API on {addr} never came up");
}

/// Collects `FAILED: <TestId>` lines from the TCK container's own stdout.
/// The container process itself always exits 0 regardless of test
/// outcome — watching its logs (exactly like the upstream
/// `dataplane-sdk-rust`'s own `TckTestReporter` in
/// `crates/sdk-tck-tests/tests/util.rs` does) is the only way to learn
/// which tests actually failed.
#[derive(Clone, Default)]
struct TckReporter {
    failures: Arc<Mutex<Vec<String>>>,
}

static FAIL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"FAILED: (\w+:\d+-\d+)").expect("valid regex"));

impl LogConsumer for TckReporter {
    fn accept<'a>(&'a self, record: &'a LogFrame) -> BoxFuture<'a, ()> {
        let log = String::from_utf8_lossy(record.bytes());
        if let Some(caps) = FAIL_REGEX.captures(&log) {
            self.failures
                .lock()
                .expect("failures lock poisoned")
                .push(caps[1].to_string());
        }
        print!("{log}");
        futures::future::ready(()).boxed()
    }
}

impl TckReporter {
    fn failures(&self) -> Vec<String> {
        self.failures
            .lock()
            .expect("failures lock poisoned")
            .clone()
    }
}
