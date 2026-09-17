use std::path::PathBuf;

/// One file exposed as a dataspace data offer: the file path/media type
/// (DCAT-shaped) and its usage policy (ODRL-shaped), as authored input to
/// [`crate::ConfigGraph::seed_offer`].
///
/// This is *input*, not the store's own representation — `seed_offer`
/// decomposes it into real RDF triples (see the crate-level docs on
/// `ds:order`); nothing downstream reads a `FileOffer` again.
#[derive(Debug, Clone)]
pub struct FileOffer {
    /// Used to build every IRI for this offer and as the DPS `datasetId`.
    pub dataset_id: String,
    pub title: String,
    pub file_path: PathBuf,
    pub media_type: String,
    /// `odrl:assigner` on the policy — the participant offering the file.
    pub assigner: String,
    pub permissions: Vec<PermissionSpec>,
}

/// One `odrl:permission` entry: an action, optionally gated by constraints.
#[derive(Debug, Clone)]
pub struct PermissionSpec {
    /// A bare ODRL action term, e.g. `"use"`.
    pub action: String,
    pub constraints: Vec<ConstraintSpec>,
}

/// One `odrl:constraint` entry (`leftOperand operator rightOperand`), e.g.
/// `dateTime lteq 2026-12-31T23:59:59Z`.
#[derive(Debug, Clone)]
pub struct ConstraintSpec {
    pub left_operand: String,
    pub operator: String,
    pub right_operand: String,
}
