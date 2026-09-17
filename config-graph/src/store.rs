use std::path::PathBuf;

use oxigraph::model::{BlankNode, GraphName, Literal, NamedNode, Quad, Term};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;

use crate::offer::FileOffer;
use crate::vocab::{dcat, dcterms, ds, odrl, rdf, xsd};

#[derive(Debug, thiserror::Error)]
pub enum ConfigGraphError {
    #[error("invalid IRI: {0}")]
    Iri(#[from] oxigraph::model::IriParseError),
    #[error("store error: {0}")]
    Storage(#[from] oxigraph::store::StorageError),
    #[error("SPARQL query failed: {0}")]
    Query(#[from] oxigraph::sparql::QueryEvaluationError),
    #[error("SPARQL query failed to parse: {0}")]
    Syntax(#[from] oxigraph::sparql::SparqlSyntaxError),
    #[error("no offer found for dataset id '{0}'")]
    NotFound(String),
}

/// The configuration graph: an embedded, physically-local Oxigraph store
/// holding one triple per fact about a file offer — its DCAT dataset/
/// distribution description and its ODRL usage policy, as real RDF, not a
/// serialized blob. Isolated from anything the Data Plane Signaling layer
/// persists (that lifecycle state lives in `dataplane-sdk`'s own
/// `DataFlowRepo`, entirely separately) — the same physical-separation
/// pattern Contreforts uses between its config graph and its
/// knowledge-graph store, applied here to config-vs-signaling-state
/// instead of config-vs-synced-data.
pub struct ConfigGraph {
    store: Store,
}

/// The IRI identifying a dataset, and the value `odrl:target` is set to —
/// also what a caller must pass as the asset identifier to
/// `dsp_odrl_adapter::request_for` (as its `dataset_id` argument) so the
/// engine's target match succeeds. Distinct from the plain `dataset_id`
/// slug used in HTTP paths and `DataFlow::dataset_id`.
pub fn dataset_iri(dataset_id: &str) -> String {
    format!("urn:ds-sql-dps-rs:dataset:{dataset_id}")
}

fn distribution_iri(dataset_id: &str) -> String {
    format!("{}#distribution", dataset_iri(dataset_id))
}

fn policy_iri(dataset_id: &str) -> String {
    format!("{}#policy", dataset_iri(dataset_id))
}

fn literal_value(term: &Term) -> Option<String> {
    match term {
        Term::Literal(l) => Some(l.value().to_string()),
        Term::NamedNode(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

impl ConfigGraph {
    /// Opens a fresh, empty, in-process store. MVP scope is one file
    /// offer per process; there is no on-disk persistence requirement
    /// this small a config warrants yet (mirrors the "in-memory is fine
    /// for a demo" call already made for DPS lifecycle state).
    pub fn open_in_memory() -> Result<Self, ConfigGraphError> {
        Ok(Self {
            store: Store::new()?,
        })
    }

    /// Decomposes a [`FileOffer`] into RDF triples: `dcat:Dataset` /
    /// `dcat:Distribution` for the file's metadata, and a full
    /// `odrl:Offer` policy graph — permissions and their constraints as
    /// real nodes, each carrying an explicit `ds:order` so
    /// [`Self::policy_jsonld`] can reconstruct the ordered JSON arrays
    /// `dsp-odrl-adapter` needs back out.
    pub fn seed_offer(&self, offer: &FileOffer) -> Result<(), ConfigGraphError> {
        let dataset = NamedNode::new(dataset_iri(&offer.dataset_id))?;
        let distribution = NamedNode::new(distribution_iri(&offer.dataset_id))?;
        let policy = NamedNode::new(policy_iri(&offer.dataset_id))?;

        let mut quads = vec![
            Quad::new(
                dataset.clone(),
                NamedNode::new(rdf("type"))?,
                NamedNode::new(dcat("Dataset"))?,
                GraphName::DefaultGraph,
            ),
            Quad::new(
                dataset.clone(),
                NamedNode::new(dcterms("title"))?,
                Literal::new_simple_literal(&offer.title),
                GraphName::DefaultGraph,
            ),
            Quad::new(
                dataset.clone(),
                NamedNode::new(dcat("distribution"))?,
                distribution.clone(),
                GraphName::DefaultGraph,
            ),
            Quad::new(
                distribution.clone(),
                NamedNode::new(rdf("type"))?,
                NamedNode::new(dcat("Distribution"))?,
                GraphName::DefaultGraph,
            ),
            Quad::new(
                distribution.clone(),
                NamedNode::new(ds("filePath"))?,
                Literal::new_simple_literal(offer.file_path.to_string_lossy()),
                GraphName::DefaultGraph,
            ),
            Quad::new(
                distribution.clone(),
                NamedNode::new(dcat("mediaType"))?,
                Literal::new_simple_literal(&offer.media_type),
                GraphName::DefaultGraph,
            ),
            Quad::new(
                policy.clone(),
                NamedNode::new(rdf("type"))?,
                NamedNode::new(odrl("Offer"))?,
                GraphName::DefaultGraph,
            ),
            Quad::new(
                policy.clone(),
                NamedNode::new(odrl("target"))?,
                // The *dataset*, not the distribution: this is what a
                // caller's `dataset_id`/asset identifier must match
                // (`Self::dataset_iri`), matching real DSP usage where a
                // contract's `odrl:target` names the Asset, not one of
                // its distributions.
                dataset.clone(),
                GraphName::DefaultGraph,
            ),
            Quad::new(
                policy.clone(),
                NamedNode::new(odrl("assigner"))?,
                Literal::new_simple_literal(&offer.assigner),
                GraphName::DefaultGraph,
            ),
        ];

        let integer = NamedNode::new(xsd("integer"))?;

        for (i, permission) in offer.permissions.iter().enumerate() {
            let p = BlankNode::default();
            quads.push(Quad::new(
                policy.clone(),
                NamedNode::new(odrl("permission"))?,
                p.clone(),
                GraphName::DefaultGraph,
            ));
            quads.push(Quad::new(
                p.clone(),
                NamedNode::new(rdf("type"))?,
                NamedNode::new(odrl("Permission"))?,
                GraphName::DefaultGraph,
            ));
            quads.push(Quad::new(
                p.clone(),
                NamedNode::new(ds("order"))?,
                Literal::new_typed_literal(i.to_string(), integer.clone()),
                GraphName::DefaultGraph,
            ));
            quads.push(Quad::new(
                p.clone(),
                NamedNode::new(odrl("action"))?,
                Literal::new_simple_literal(&permission.action),
                GraphName::DefaultGraph,
            ));

            for (j, constraint) in permission.constraints.iter().enumerate() {
                let c = BlankNode::default();
                quads.push(Quad::new(
                    p.clone(),
                    NamedNode::new(odrl("constraint"))?,
                    c.clone(),
                    GraphName::DefaultGraph,
                ));
                quads.push(Quad::new(
                    c.clone(),
                    NamedNode::new(ds("order"))?,
                    Literal::new_typed_literal(j.to_string(), integer.clone()),
                    GraphName::DefaultGraph,
                ));
                quads.push(Quad::new(
                    c.clone(),
                    NamedNode::new(odrl("leftOperand"))?,
                    Literal::new_simple_literal(&constraint.left_operand),
                    GraphName::DefaultGraph,
                ));
                quads.push(Quad::new(
                    c.clone(),
                    NamedNode::new(odrl("operator"))?,
                    Literal::new_simple_literal(&constraint.operator),
                    GraphName::DefaultGraph,
                ));
                quads.push(Quad::new(
                    c,
                    NamedNode::new(odrl("rightOperand"))?,
                    Literal::new_simple_literal(&constraint.right_operand),
                    GraphName::DefaultGraph,
                ));
            }
        }

        for quad in &quads {
            self.store.insert(quad)?;
        }
        Ok(())
    }

    fn select(
        &self,
        query: &str,
    ) -> Result<Vec<oxigraph::sparql::QuerySolution>, ConfigGraphError> {
        let results = SparqlEvaluator::new()
            .parse_query(query)?
            .on_store(&self.store)
            .execute()?;
        let QueryResults::Solutions(solutions) = results else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for solution in solutions {
            out.push(solution?);
        }
        Ok(out)
    }

    /// The distribution's local file path, read back from the graph.
    pub fn file_path(&self, dataset_id: &str) -> Result<PathBuf, ConfigGraphError> {
        let distribution = distribution_iri(dataset_id);
        let query = format!(
            r#"PREFIX ds: <{DS}>
               SELECT ?path WHERE {{ <{distribution}> ds:filePath ?path }}"#,
            DS = crate::vocab::DS,
        );
        let solutions = self.select(&query)?;
        let path = solutions
            .first()
            .and_then(|s| s.get("path"))
            .and_then(literal_value)
            .ok_or_else(|| ConfigGraphError::NotFound(dataset_id.to_string()))?;
        Ok(PathBuf::from(path))
    }

    /// The distribution's media type, read back from the graph.
    pub fn media_type(&self, dataset_id: &str) -> Result<String, ConfigGraphError> {
        let distribution = distribution_iri(dataset_id);
        let query = format!(
            r#"PREFIX dcat: <{DCAT}>
               SELECT ?media WHERE {{ <{distribution}> dcat:mediaType ?media }}"#,
            DCAT = crate::vocab::DCAT,
        );
        let solutions = self.select(&query)?;
        solutions
            .first()
            .and_then(|s| s.get("media"))
            .and_then(literal_value)
            .ok_or_else(|| ConfigGraphError::NotFound(dataset_id.to_string()))
    }

    /// Reconstructs the dataset's ODRL policy as an ODRL JSON-LD document
    /// — real vocabulary terms under the W3C `odrl.jsonld` context,
    /// permission/constraint arrays rebuilt in their recorded `ds:order` —
    /// suitable for `dsp_odrl_adapter::ingest_policy_value`.
    pub fn policy_jsonld(&self, dataset_id: &str) -> Result<serde_json::Value, ConfigGraphError> {
        let policy = policy_iri(dataset_id);
        let odrl_ns = crate::vocab::ODRL;
        let ds_ns = crate::vocab::DS;

        let header_query = format!(
            r#"PREFIX odrl: <{odrl_ns}>
               SELECT ?target ?assigner WHERE {{
                   <{policy}> odrl:target ?target ; odrl:assigner ?assigner .
               }}"#
        );
        let header = self.select(&header_query)?;
        let header = header
            .first()
            .ok_or_else(|| ConfigGraphError::NotFound(dataset_id.to_string()))?;
        let target = header
            .get("target")
            .and_then(literal_value)
            .unwrap_or_default();
        let assigner = header
            .get("assigner")
            .and_then(literal_value)
            .unwrap_or_default();

        // One joined, fully-ordered query rather than a per-permission
        // follow-up query: a blank node's label in SPARQL query *syntax*
        // is scoped to that query and never matches a stored blank node
        // by identity, so "query again using the id I just read back" is
        // not a valid way to re-address a specific permission node. Rows
        // are grouped by `?p` in Rust below instead, using the already
        // permission-then-constraint `ds:order`ed result.
        let rows_query = format!(
            r#"PREFIX odrl: <{odrl_ns}>
               PREFIX ds: <{ds_ns}>
               SELECT ?p ?porder ?action ?corder ?left ?op ?right WHERE {{
                   <{policy}> odrl:permission ?p .
                   ?p ds:order ?porder ; odrl:action ?action .
                   OPTIONAL {{
                       ?p odrl:constraint ?c .
                       ?c ds:order ?corder ; odrl:leftOperand ?left ; odrl:operator ?op ; odrl:rightOperand ?right .
                   }}
               }} ORDER BY ?porder ?corder"#
        );

        let mut permissions: Vec<(Term, serde_json::Value)> = Vec::new();
        for row in self.select(&rows_query)? {
            let Some(p) = row.get("p").cloned() else {
                continue;
            };
            let action = row
                .get("action")
                .and_then(literal_value)
                .unwrap_or_default();

            let entry = match permissions.last_mut() {
                Some((last_p, value)) if *last_p == p => value,
                _ => {
                    permissions
                        .push((p, serde_json::json!({ "action": action, "constraint": [] })));
                    &mut permissions.last_mut().expect("just pushed").1
                }
            };

            if let Some(left) = row.get("left").and_then(literal_value) {
                entry["constraint"].as_array_mut().expect("constraint is an array").push(serde_json::json!({
                    "leftOperand": left,
                    "operator": row.get("op").and_then(literal_value).unwrap_or_default(),
                    "rightOperand": row.get("right").and_then(literal_value).unwrap_or_default(),
                }));
            }
        }

        let permissions: Vec<serde_json::Value> = permissions
            .into_iter()
            .map(|(_, mut value)| {
                if value["constraint"].as_array().is_some_and(Vec::is_empty) {
                    value
                        .as_object_mut()
                        .expect("permission is an object")
                        .remove("constraint");
                }
                value
            })
            .collect();

        Ok(serde_json::json!({
            "@context": "http://www.w3.org/ns/odrl.jsonld",
            "@id": policy,
            "@type": "Offer",
            "target": target,
            "assigner": assigner,
            "permission": permissions,
        }))
    }
}
