//! The file offer's configuration graph: an embedded Oxigraph RDF store
//! describing a local file as a DCAT dataset/distribution with an ODRL
//! usage policy, addressed by IRI and queried via SPARQL — real RDF
//! triples, not a serialized blob, so the policy stays SPARQL-queryable
//! and the file's metadata lives in one auditable place.

pub mod offer;
mod store;
pub mod vocab;

pub use offer::{ConstraintSpec, FileOffer, PermissionSpec};
pub use store::{ConfigGraph, ConfigGraphError, dataset_iri};
