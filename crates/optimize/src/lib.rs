#![forbid(unsafe_code)]

use serde_json::Value;

pub mod admit;
pub mod atom;
pub mod cachecost;
pub mod certificate;
pub mod clmh;
mod column;
pub mod columnar;
pub mod counterfactual;
pub mod dedup;
pub mod detectorgate;
pub mod dict;
pub mod distilled;
pub mod doc;
pub mod frontier;
pub mod frontlines;
pub mod format_condenser;
pub mod grouped;
pub mod kv;
pub mod lines;
pub mod lock;
pub mod log;
pub mod nested;
pub mod netcost;
pub mod norm;
pub mod offload;
pub mod outline;
pub mod prefix;
pub mod prose;
pub mod proxy;
pub mod reconcile;
pub mod recordcol;
pub mod relevance;
pub mod replay;
pub mod resolve;
pub mod richness;
pub mod search;
pub mod select;
pub mod shape;
pub mod shred;
pub mod text_columnar;
pub mod tokens;
pub mod transform;
pub mod tree;
pub mod variables_simplifier;
pub mod yaml_condenser;
#[cfg(feature = "treesitter")]
pub mod treesit;

pub use atom::canonicalize;

// Atom count + blake3 fidelity certificate of a raw block, so every surface records the same proof.
pub fn proof(raw: &str) -> (u64, String) {
    let atoms = serde_json::from_str::<Value>(raw)
        .map(|value| atom::leaves(&value).len() as u64)
        .unwrap_or_else(|| raw.split_whitespace().count() as u64);
    (atoms, certificate::certify(raw).hash)
}

use std::sync::Arc;

use admit::{Certificate, Refusal, admit};
use columnar::Columnar;
use doc::Doc;
use nested::Nested;
use netcost::{NetCostGate, Verdict, Zone};
use norm::Normalize;
use offload::{OffloadStore, Store};
use tokens::{ByteCounter, TokenCounter};
use transform::{Encoded, TextProposer, Transform};
use variables_simplifier::VariablesSimplifier;

type StructuredCandidate = (String, &'static str, Certificate, f64, usize);
type StructuredSearch = (Option<StructuredCandidate>, Option<KeptReason>);

pub enum Outcome {
    Compressed {
        wire: String,
        transform: &'static str,
        certificate: Certificate,
        saved_usd: f64,
    },
    Offloaded {
        stub: String,
        marker: String,
        saved_usd: f64,
    },
    KeptVerbatim {
        reason: KeptReason,
    },
}

pub enum KeptReason {
    NotApplicable,
    Refused(&'static str, Refusal),
    NoNetSaving(netcost::Reason),
    DetectorFired,
    DuplicateKeys,
}

impl KeptReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeptReason::NotApplicable => "not_applicable",
            KeptReason::Refused(_, Refusal::ClmhMismatch) => "refused_clmh",
            KeptReason::Refused(_, Refusal::InverseWitnessFailed) => "refused_witness",
            KeptReason::Refused(_, Refusal::NotIdempotent) => "refused_idempotency",
            KeptReason::NoNetSaving(netcost::Reason::FrozenWriteWouldExceedRead) => "no_saving_frozen",
            KeptReason::NoNetSaving(netcost::Reason::NotWorthIt) => "no_saving_notworth",
            KeptReason::NoNetSaving(netcost::Reason::UnknownModel) => "no_saving_unknown_model",
            KeptReason::DetectorFired => "detector",
            KeptReason::DuplicateKeys => "duplicate_keys",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OffloadMode { Off, #[default] Auto, Always }

const REOPEN_PRIOR: f64 = 0.45;
const RELEVANCE_MIN_ROWS: usize = 8;
const RELEVANCE_MAX_INLINE: usize = 24;
const PROSE_SUMMARY_MIN_BYTES: usize = 1000;
const PROSE_SUMMARY_FRACTION: f64 = 0.5;
const BUILTIN_TRANSFORM_COUNT: usize = 5;

pub struct Optimizer {
    transforms: Vec<Box<dyn Transform>>,
    text_proposers: Vec<Box<dyn TextProposer>>,
    proposers_enabled: bool,
    gate: NetCostGate,
    zone: Zone,
    store: Arc<dyn OffloadStore>,
    counter: Arc<dyn TokenCounter>,
    embedder: Arc<dyn relevance::Embedder>,
    prose_mode: bool,
    prose_shrinker: Option<Arc<dyn prose::ProseShrinker>>,
    offload_mode: OffloadMode,
}

impl Default for Optimizer {
    fn default() -> Self { Self::new(NetCostGate::new("claude-sonnet-4-5", 1), Zone::Suffix) }
}

impl Optimizer {
    pub fn new(gate: NetCostGate, zone: Zone) -> Self {
        let counter: Arc<dyn TokenCounter> = Arc::new(ByteCounter);
        Self {
            transforms: Self::built_in_transforms(counter.clone()),
            // Format proposers are model-facing candidates. Each must prove its own reconstruction
            // and then pass the normal detector + real token net-cost gate before shipping.
            text_proposers: vec![
                Box::new(yaml_condenser::YamlCondenser),
                Box::new(format_condenser::FormatCondenser),
            ],
            proposers_enabled: true,
            gate,
            zone,
            store: Arc::new(Store::default()),
            counter,
            embedder: Arc::new(distilled::DistilledEmbedder),
            prose_mode: false,
            prose_shrinker: None,
            offload_mode: OffloadMode::Auto,
        }
    }

    fn built_in_transforms(counter: Arc<dyn TokenCounter>) -> Vec<Box<dyn Transform>> {
        vec![Box::new(Columnar::with_counter(counter.clone())), Box::new(Normalize::with_counter(counter.clone())), Box::new(Nested::with_counter(counter.clone())), Box::new(Doc::with_counter(counter.clone())), Box::new(VariablesSimplifier)]
    }

    pub fn set_offload_mode(&mut self, mode: OffloadMode) { self.offload_mode = mode; }
    pub fn offload_mode(&self) -> OffloadMode { self.offload_mode }
    fn eviction_off(&self) -> bool { matches!(self.offload_mode, OffloadMode::Off) }
    pub fn set_offload_allowed(&mut self, allowed: bool) { self.offload_mode = if allowed { OffloadMode::Auto } else { OffloadMode::Off }; }
