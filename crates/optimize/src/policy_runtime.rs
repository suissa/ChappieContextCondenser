//! Executes the existing format matrix under an explicit, validated codec policy.
//!
//! The caller supplies the deployment tokenizer so the runtime does not silently substitute byte
//! length or a mismatched tokenizer. Every returned wire must reconstruct the original input before
//! it is eligible for selection.

use crate::policy::{CodecFamily, CodecPolicy, FileKind, FitnessMeasurement};
use crate::format_matrix::FormatMatrix;
use crate::transform::TextProposer;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedCandidate {
    pub wire: String,
    pub token_count: u64,
    pub codec_path: Vec<CodecFamily>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyExecution {
    pub selected: Option<VerifiedCandidate>,
    pub candidates_considered: usize,
    pub candidates_rejected: usize,
    pub input_tokens: u64,
    pub latency_micros: u64,
    pub reconstruction_valid: bool,
}

impl PolicyExecution {
    pub fn measurement(&self) -> FitnessMeasurement {
        FitnessMeasurement {
            input_tokens: self.input_tokens,
            output_tokens: self.selected.as_ref().map_or(self.input_tokens, |c| c.token_count),
            latency_micros: self.latency_micros,
            reconstruction_valid: self.reconstruction_valid,
        }
    }
}

/// Execute the matrix and select the lowest-token verified candidate allowed by `policy`.
/// `count_tokens` must use the same tokenizer/model encoding as deployment and include the complete
/// wire, including codec headers and dictionaries. Unsupported file-kind/codec combinations simply
/// produce no candidate; the original text remains the safe fallback.
pub fn execute_policy<F>(
    raw: &str,
    policy: &CodecPolicy,
    mut count_tokens: F,
) -> Result<PolicyExecution, crate::policy::PolicyError>
where
    F: FnMut(&str) -> u64,
{
    policy.validate()?;
    let started = Instant::now();
    let input_tokens = count_tokens(raw);
    let matrix = FormatMatrix;
    let mut candidates_considered = 0usize;
    let mut candidates_rejected = 0usize;
    let mut best: Option<VerifiedCandidate> = None;

    if policy.max_candidates > 0 {
        for wire in matrix.encode_candidates(raw) {
            let Some(path) = codec_path(&wire) else {
                candidates_rejected += 1;
                continue;
            };
            if !path_allowed(&path, policy) {
                candidates_rejected += 1;
                continue;
            }
            candidates_considered += 1;
            if matrix.decode(&wire).as_deref() != Some(raw) {
                candidates_rejected += 1;
                continue;
            }
            let token_count = count_tokens(&wire);
            let candidate = VerifiedCandidate { wire, token_count, codec_path: path };
            let should_replace = best.as_ref().is_none_or(|current| {
                candidate.token_count < current.token_count
                    || (candidate.token_count == current.token_count && candidate.wire.len() < current.wire.len())
            });
            if should_replace {
                best = Some(candidate);
            }
            if candidates_considered >= policy.max_candidates {
                break;
            }
        }
    }

    let latency_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    Ok(PolicyExecution {
        reconstruction_valid: best.is_some(),
        selected: best,
        candidates_considered,
        candidates_rejected,
        input_tokens,
        latency_micros,
    })
}

fn path_allowed(path: &[CodecFamily], policy: &CodecPolicy) -> bool {
    if path.len() > 1 && !policy.allow_composition {
        return false;
    }
    path.iter().all(|codec| policy.enabled_codecs.contains(codec))
}

/// Parse only the stable wire markers emitted by FormatMatrix. Unknown wire formats fail closed.
fn codec_path(wire: &str) -> Option<Vec<CodecFamily>> {
    if let Some(rest) = wire.strip_prefix("SFM2\n") {
        let (chain, _) = rest.split_once('\n')?;
        return chain.split(',').map(parse_codec).collect();
    }
    if let Some(rest) = wire.strip_prefix("SFM1\n") {
        let (id, _) = rest.split_once('\n')?;
        return Some(vec![parse_codec(id)?, CodecFamily::Lines]);
    }
    if let Some(rest) = wire.strip_prefix("SFM1D\n") {
        let (id, _) = rest.split_once('\n')?;
        return Some(vec![parse_codec(id)?, CodecFamily::Dictionary]);
    }
    if wire.starts_with("SJSON1\n") || wire.starts_with("STOML1\n") {
        return Some(vec![CodecFamily::FormatCondenser]);
    }
    if wire.starts_with("SWY1\n") {
        return Some(vec![CodecFamily::YamlCondenser]);
    }
    if wire.starts_with("[lines]\n") {
        return Some(vec![CodecFamily::Lines]);
    }
    if wire.starts_with("[dict]\n") {
        return Some(vec![CodecFamily::Dictionary]);
    }
    None
}

fn parse_codec(id: &str) -> Option<CodecFamily> {
    match id {
        "format-condenser" => Some(CodecFamily::FormatCondenser),
        "yaml-condenser" => Some(CodecFamily::YamlCondenser),
        "lines" => Some(CodecFamily::Lines),
        "dict" => Some(CodecFamily::Dictionary),
        "composed-lines" | "composed-dict" | "composed-stack" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(kind: FileKind, codecs: Vec<CodecFamily>, compose: bool) -> CodecPolicy {
        CodecPolicy {
            schema_version: crate::policy::POLICY_SCHEMA_VERSION,
            file_kind: kind,
            enabled_codecs: codecs,
            allow_composition: compose,
            max_candidates: 32,
            latency_weight: 0.0,
        }
    }

    #[test]
    fn only_selects_candidates_whose_codec_family_is_enabled() {
        let raw = (0..12).map(|i| format!("service-{i} repeated-long-value repeated-long-value")).collect::<Vec<_>>().join("\n");
        let p = policy(FileKind::Logs, vec![CodecFamily::Dictionary], false);
        let result = execute_policy(&raw, &p, |s| s.len() as u64).unwrap();
        if let Some(candidate) = result.selected {
            assert_eq!(candidate.codec_path, vec![CodecFamily::Dictionary]);
            assert_eq!(FormatMatrix.decode(&candidate.wire).as_deref(), Some(raw.as_str()));
        }
    }

    #[test]
    fn rejects_composition_when_policy_disallows_it() {
        let raw = r#"{"customer_identifier":"alpha","customer_identifier_long":"beta","customer_identifier_more":"gamma"}"#;
        let p = policy(FileKind::Json, vec![CodecFamily::FormatCondenser, CodecFamily::Lines, CodecFamily::Dictionary], false);
        let result = execute_policy(raw, &p, |s| s.len() as u64).unwrap();
        assert!(result.selected.as_ref().is_none_or(|c| c.codec_path.len() == 1));
    }

    #[test]
    fn token_counter_prices_the_full_wire_and_reports_reconstruction() {
        let raw = "alpha alpha alpha alpha alpha alpha";
        let p = policy(FileKind::Logs, vec![CodecFamily::Dictionary], false);
        let result = execute_policy(raw, &p, |s| s.split_whitespace().count() as u64).unwrap();
        assert!(result.input_tokens > 0);
        assert_eq!(result.measurement().input_tokens, result.input_tokens);
        assert_eq!(result.reconstruction_valid, result.selected.is_some());
    }

    #[test]
    fn invalid_policy_is_rejected_before_execution() {
        let p = policy(FileKind::Json, vec![], false);
        assert!(execute_policy("{}", &p, |s| s.len() as u64).is_err());
    }
}
