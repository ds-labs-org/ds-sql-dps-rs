//! RDF namespace IRIs used to describe a file offer: standard DCAT (dataset
//! metadata) and ODRL (usage policy), plus one small bespoke predicate
//! (`ds:order`) this store needs of its own.
//!
//! `ds:order` exists because ODRL's `permission`/`prohibition`/`obligation`
//! and a constraint's `and`/`or`/`xone` members are *ordered* JSON arrays —
//! `dsp-odrl-adapter` (vendored at `../ds-odrl-engine-rs/dsp-odrl-adapter`)
//! reports that order back in the engine's own `reason` trace
//! (`permission[0]`, `prohibition[1]`) — but plain RDF triples for a
//! multi-valued blank-node property carry no order at all. Recording an
//! explicit integer position on each such node, and sorting by it when
//! reconstructing JSON-LD, is what recovers it.

pub const DCAT: &str = "http://www.w3.org/ns/dcat#";
pub const DCTERMS: &str = "http://purl.org/dc/terms/";
pub const ODRL: &str = "http://www.w3.org/ns/odrl/2/";
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// This project's own tiny vocabulary: the file offer's storage-facing
/// metadata (a local path) that has no DCAT term, plus `order`.
pub const DS: &str = "https://ds42.org/ontologies/ds-sql-dps-rs#";

pub fn odrl(term: &str) -> String {
    format!("{ODRL}{term}")
}

pub fn dcat(term: &str) -> String {
    format!("{DCAT}{term}")
}

pub fn dcterms(term: &str) -> String {
    format!("{DCTERMS}{term}")
}

pub fn ds(term: &str) -> String {
    format!("{DS}{term}")
}

pub fn rdf(term: &str) -> String {
    format!("{RDF}{term}")
}

pub fn xsd(term: &str) -> String {
    format!("{XSD}{term}")
}
