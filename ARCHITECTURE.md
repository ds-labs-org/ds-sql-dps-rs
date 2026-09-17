# Architecture

<img src="https://avatars.githubusercontent.com/u/250248?v=4" alt="Nicolas Karageuzian" width="40" height="40" style="border-radius:50%;vertical-align:middle;margin-right:10px;" /> **Nicolas Karageuzian** ([nka11](https://github.com/nka11)) — [ds42.org](https://ds42.org)
**Document type:** Architecture record (submodule-local — not a `ds42.org` ADR)

*Written with AI-agent assistance (Claude Code) under the author's
direction and review, following the workflow used across the `ds42.org`
study repos this project is developed alongside.*

- **Status:** MVP, working end to end (see "What has actually been run"
  below). Not yet integrated with a live EDC control plane or a running
  Contreforts product.
- **Date:** 2026-09-17

This file records the scope and design decisions behind this project's
MVP, and why each was made — a submodule-local architecture record rather
than a `ds42.org`-repo ADR, spike, or case study, by the maintainer's own
choice for this project's first cut.

## What this project is

`ds-sql-dps-rs` exposes a single local file as a dataspace **data
offer**: it implements the [EDC Data Plane Signaling
API](https://eclipse-edc.github.io/documentation/for-adopters/data-plane/#data-plane-signaling)
(the control-plane-facing protocol that drives a data plane through
START/SUSPEND/TERMINATE), describes the file with real DCAT (dataset)
and ODRL (usage policy) vocabulary stored as RDF in an embedded
configuration graph, and exposes that graph through
[Contreforts](https://github.com/contreforts-ai)' `ContrefortsConnector`
trait — the semantic-configuration-layer interface this project was
asked to use.

"SQL" in the name is aspirational for this MVP: see "SQL scope" below.

## Decisions

### Contreforts coupling: in-process trait implementation

**Decision:** vendor `contreforts-core` (`vendor/contreforts-core`, a git
submodule, no tag available upstream — pinned to a specific commit) and
implement `ContrefortsConnector` directly in `contreforts-connector/`,
following the precedent already set by `ds-catalog-broker-rs` (which
vendors the same crate family the same way).

**Why:** Contreforts' public org (github.com/contreforts-ai) turned out,
on inspection, to be an ERP/business-data-sync and RAG toolkit — there is
no DCAT, ODRL, or Dataspace Protocol anywhere in its public code.
`ContrefortsConnector` is a generic adapter trait (`pull`/`push`/`get`/
`fetch_content`, plus a Turtle `declaration_ttl()` self-description
validated by SHACL) designed for wrapping a business SaaS system, not a
dataspace participant. What *is* directly reusable is the pattern: a
physically separate, SHACL-declared, SPARQL-addressable configuration
graph, isolated from whatever store holds synced/lifecycle data. This
project reuses the trait and declaration mechanism, and mints its own
`EntityKind` (`ds-sql-dps-rs:data-offer`) and namespace
(`contreforts-connector/src/declaration.ttl`) rather than forcing itself
into Contreforts' business vocabulary — `EntityKind` is explicitly an
open, extensible type built for exactly this.

**What's unverified:** how a connector actually gets wired into a
*running* Contreforts product is handled by `contreforts-product`, which
is private. This project's own binary calls
`FileOfferConnector::get()` once at startup to prove the round trip
compiles and returns real data (see `dataplane/src/main.rs`), but nothing
here has been run against an actual Contreforts deployment.

### Offer vocabulary: standard DCAT + ODRL, evaluated via `ds-odrl-engine-rs`

**Decision:** the file is described as `dcat:Dataset`/`dcat:Distribution`
and its usage policy as a real `odrl:Offer` — standard, interoperable
dataspace vocabulary, not a Contreforts-style bespoke one. Policy
evaluation reuses the already-vendored `ds-odrl-engine-rs` (sibling
submodule `../ds-odrl-engine-rs`), specifically its `dsp-odrl-adapter`
crate (`dsp-ingest` feature), which ingests real ODRL JSON-LD (as carried
in a DSP contract offer/agreement) into the engine's `WirePolicy` — see
`docs/spikes/2026-09-05-odrl-2.2-vocabulary-gap-analysis.md` and
`dsp-odrl-adapter/README.md` in the main `dataspace` repo for what that
ingestion path does and does not cover. `dataplane/src/public.rs` and
`dataplane/src/main.rs` (via `contreforts-connector`) both build the
policy document through `config_graph::ConfigGraph::policy_jsonld`, feed
it to `dsp_odrl_adapter::ingest_policy_value`, and evaluate with
`engine::evaluate_request`.

### Policy storage shape: full RDF triples, not a JSON-LD literal

**Decision:** the ODRL policy is decomposed into real RDF triples in the
Oxigraph configuration graph (`config-graph/src/store.rs`) — permissions
and constraints as blank nodes with an explicit `ds:order` integer
predicate — and reconstructed into ordered ODRL JSON-LD on demand
(`ConfigGraph::policy_jsonld`), rather than stored as an opaque JSON-LD
string literal.

**Why, and the real cost of it:** this keeps the policy SPARQL-queryable
like the rest of the configuration graph, at the cost of solving a real
problem: `dsp-odrl-adapter` reports `permission[0]`/`prohibition[1]`
array-*order* in the engine's own `reason` trace, but plain RDF triples
for a multi-valued blank-node property carry no order at all — the
crate's own README explicitly rejected `oxjsonld`-based RDF decomposition
for exactly this reason. `ds:order` and an `ORDER BY` in the
reconstruction query is this project's answer. One concrete pitfall
found and fixed during implementation: SPARQL blank-node syntax
(`_:label`) in a *query string* is scoped to that query and never
addresses a stored blank node by identity — a naive "query once for
permissions, then re-query per permission by its blank-node id" approach
looks plausible but silently matches nothing. `policy_jsonld` instead
runs one joined, fully `ORDER BY`'d query and groups rows in Rust by
comparing the actual `Term` values.

### Enforcement timing: per GET request, not just at START

**Decision:** `dataplane/src/handler.rs`'s `on_start` mints a bearer
token unconditionally (contract negotiation already gated the agreement
one layer up); `dataplane/src/public.rs`'s `GET /public/{dataset_id}`
re-ingests the policy and re-runs `engine::evaluate_request` — with a
freshly computed `dateTime` claim — on **every** request that token
authorizes.

**Why:** a time-bound constraint (`dateTime lteq ...`) or any other
claim-based one is only meaningfully enforced if it is checked at the
moment of access, not only once when the token happened to be issued.
Verified live: with `POLICY_NOT_AFTER` set to a past timestamp, START
still succeeds (per the decision above) but the following GET is denied
with the engine's own reason string.

### SQL scope for the MVP: raw bytes, "SQL" is aspirational

**Decision:** the public endpoint streams the configured file's raw
bytes with its configured media type — it does not embed a SQL engine or
execute queries against the file. `ds-sql-dps-rs`'s name describes where
this project is headed (a SQL-queryable data-plane driver), not what this
MVP implements.

### Signaling scope: depend on `dataplane-sdk`/`dataplane-sdk-axum`, START + TERMINATE only

**Decision:** depend directly on `eclipse-dataplane-core`'s
`dataplane-sdk` and `dataplane-sdk-axum` crates (pinned to the exact
version `=0.1.2`, since both are pre-1.0) rather than hand-rolling the
signaling HTTP surface and DPS state machine. See
`docs/spikes/2026-09-17-dps-rust-implementations-for-ds-sql-dps-rs.md`
in the main `dataspace` repo for the survey that found it: a real,
actively-maintained, Eclipse Foundation project whose own CI runs the
official `eclipsedataspacetck/dps-tck-runtime` conformance image against
it. `dataplane-sdk-postgres` is deliberately **not** used — that spike
found a confirmed bug (`PgDataFlowRepo::update` only persists the `state`
column, silently dropping `data_address` on update) — the MVP uses the
SDK's in-memory repos instead, which is sufficient for a single-file,
single-process demo.

Within that SDK, this project implements only the provider role's
required hooks meaningfully (`on_start`, `on_terminate`); `on_prepare`
and `on_suspend` return `HandlerError::NotSupported` rather than a
silent no-op, so a caller cannot mistake "not implemented" for "it
worked." `on_started` is a no-op (the `DataAddress`/token are already
returned synchronously from `on_start`). This is deliberately narrower
than what the SDK's router actually exposes — all eight DPS routes are
mounted regardless, since `dataplane-sdk-axum::router::router()` wires
them as one unit — but the MVP's own logic only ever does real work for
two of them.

### Vendoring and placement: root-level submodule, sibling to `site/`

**Decision:** `ds-sql-dps-rs` is a root-level git submodule of the
`dataspace` repo (sibling to `site/`, `ds-odrl-engine-rs`,
`ds-catalog-broker-rs`) rather than living under `vendor/`, and depends
on `ds-odrl-engine-rs`'s `engine`/`dsp-odrl-adapter` crates as relative
Cargo path dependencies (`../ds-odrl-engine-rs/engine`, matching how
`site/` itself consumes `engine`). `vendor/contreforts-core` is this
project's *own* vendored submodule, following the same pattern
`ds-catalog-broker-rs` already uses for the same crate family.

## What this MVP does not do

- **Control-plane self-registration is one-shot, not a heartbeat.**
  `dataplane/src/registration.rs` implements the EDC Data Plane Signaling
  v5beta self-registration call (`PUT
  /v5beta/participants/{participantContextId}/dataplanes`, ground-truthed
  against the vendored `eclipse-edc-connector` Java source — see that
  module's doc comment), and `dataplane::build` attempts it exactly once
  at startup, only when `CONTROL_PLANE_URL` is set (`dataplane/src/config.rs`).
  It is deliberately **optional and non-fatal**: with `CONTROL_PLANE_URL`
  unset, nothing changes from before (no request is made, no log beyond an
  info line); if set and the PUT fails (unreachable control plane, or a
  non-2xx response — `RegistrationError` distinguishes the two), that is a
  `tracing::warn!`, not a startup failure. What's still missing: no retry,
  no periodic re-registration, and no `allowedSourceTypes`/health-based
  deregistration — a real long-lived data plane would need at least
  periodic re-announcement, which this MVP does not attempt.
- **No SUSPEND, no provider-push.** Both return `HandlerError::NotSupported`.
- **No SQL execution** — see "SQL scope" above.
- **No durable storage.** Both the configuration graph and the DPS
  lifecycle state are in-memory; everything is reseeded from environment
  variables (`dataplane/src/config.rs`) on each process start.
- **`dsp-odrl-adapter`'s documented ingestion gaps apply as-is**:
  `odrl:andSequence`, `odrl:inheritFrom`, and `odrl:conflict` are not
  mapped from an ingested policy. Accepted as a known limitation for
  this MVP (see the adapter's own README) rather than a blocker —
  policies authored for this project simply avoid those three
  constructs.
- **`ContrefortsConnector` registration into a running product** is
  unconfirmed — see "Contreforts coupling" above.

## What has actually been run

Built and exercised locally end to end (`cargo run -p dataplane`), not
just compiled:

1. `POST /api/v1/dataflows/start` with a real `DataFlowStartMessage`
   returns a `DataFlowStatusMessage` with a `DataAddress` carrying a
   fresh bearer token.
2. `GET /public/{dataset_id}` with that token streams the configured
   file's bytes with the configured media type.
3. `GET` with a wrong dataset, or no token, is rejected (403/401).
4. `POST .../terminate` revokes the token; the same `GET` that worked in
   step 2 then returns 401.
5. With `POLICY_NOT_AFTER` set to a past timestamp, START still succeeds
   but the following `GET` is denied by a live `engine::evaluate_request`
   call, with the engine's own reason string in the response body — the
   concrete proof that policy enforcement is real and per-request, not a
   stub.
6. `cargo test --workspace` passes, including
   `contreforts-connector`'s test that its `declaration.ttl` validates
   against Contreforts' own real SHACL meta-shapes
   (`contreforts_declaration::validate`).
7. Control-plane self-registration (`dataplane/src/registration.rs`):
   `dataplane/tests/control_plane_registration.rs` mounts a `wiremock`
   mock control plane expecting exactly one `PUT
   /v5beta/participants/{participantContextId}/dataplanes` with the
   `dataplaneId`/`endpoint`/`transferTypes` this project sends, and
   passes. Additionally verified manually against a plain
   `http.server`-based mock control plane (not wiremock) on
   `127.0.0.1:18080`: running the real binary with `CONTROL_PLANE_URL` set
   produced the exact expected PUT body and path at the mock, logged as
   `"registered with control plane"`; with `CONTROL_PLANE_URL` unset,
   startup is unchanged (an info line, no request attempted); with
   `CONTROL_PLANE_URL` pointed at a closed port, startup still completes
   and logs a `tracing::warn!` naming the connection failure rather than
   aborting.

Not run: anything against a live EDC control plane, a live Contreforts
product, or the official `dps-tck` conformance suite itself (a natural
next step, following the pattern `ds-odrl-engine-rs/dsp-odrl-adapter`'s
own upstream, `dataplane-sdk-rust`, already uses for its own conformance
claim).

## Layout

```
ds-sql-dps-rs/
  config-graph/            RDF configuration graph: DCAT dataset/distribution
                            + ODRL policy as real triples, SPARQL-backed.
  contreforts-connector/   ContrefortsConnector impl over config-graph,
                            plus this connector's own declaration.ttl.
  dataplane/               The binary: wires dataplane-sdk's signaling
                            router, the public file endpoint, and
                            per-request ODRL enforcement together.
  vendor/contreforts-core/ Vendored git submodule (unpublished upstream).
  sample-data/sample.csv   Default demo file (see dataplane/src/config.rs).
```

## Running it

```sh
git submodule update --init --recursive
cargo run -p dataplane
# Signaling API:  http://localhost:9191/api/v1/dataflows/...
# Public data:    http://localhost:9192/public/{dataset_id}
```

Configuration is via environment variables (`dataplane/src/config.rs`):
`DATASET_ID`, `FILE_PATH`, `MEDIA_TYPE`, `ASSIGNER`, `POLICY_NOT_AFTER`,
`SIGNALING_PORT`, `PUBLIC_PORT`, `PUBLIC_BASE_URL`,
`PARTICIPANT_CONTEXT_ID`, `DATAPLANE_ID`, `CONTROL_PLANE_URL` (unset by
default — see "What this MVP does not do" for what setting it triggers).
