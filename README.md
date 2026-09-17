# ds-sql-dps-rs

Exposes a local file as a dataspace data offer: implements the [EDC Data
Plane Signaling API](https://eclipse-edc.github.io/documentation/for-adopters/data-plane/#data-plane-signaling)
(START/TERMINATE), describes the file with real DCAT/ODRL vocabulary
stored as RDF in an embedded configuration graph, and exposes that graph
through [Contreforts](https://github.com/contreforts-ai)' connector
interface.

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for what this MVP does, why it
was built this way, and what it deliberately does not do yet.

Part of the [ds42.org](https://ds42.org) dataspace experimentation hub —
see that repo's own `docs/spikes/2026-09-17-dps-rust-implementations-for-ds-sql-dps-rs.md`
for the survey that led to this project's dependency on `dataplane-sdk`.

## Running

```sh
git submodule update --init --recursive
cargo run -p dataplane
```

See `ARCHITECTURE.md`'s "Running it" section for endpoints and
configuration.

## Layout

- `config-graph/` — the RDF configuration graph (DCAT + ODRL, real
  triples, SPARQL-backed).
- `contreforts-connector/` — `ContrefortsConnector` implementation over
  `config-graph`.
- `dataplane/` — the binary: signaling API, public file endpoint,
  per-request ODRL enforcement.
- `vendor/contreforts-core/` — vendored git submodule.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
