//! Versioned, per-file-type codec policy genomes and a measurable fitness contract.
//!
//! This module separates policy search from codec execution: a host decodes a policy, runs enabled
//! candidate families, verifies reconstruction, measures tokenizer cost and latency, then supplies
//! those measurements to the objective.

use crate::swarm::{BinarySwarm, BinarySwarmConfig, BinarySwarmResult};
use serde::{Deserialize, Serialize};

pub const POLICY_SCHEMA_VERSION: u16 = 1;
pub const POLICY_GENOME_BITS: usize = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Json, Yaml, Toml, Csv, Xml, Html, Markdown, Source, Logs, Unknown,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json", Self::Yaml => "yaml", Self::Toml => "toml",
            Self::Csv => "csv", Self::Xml => "xml", Self::Html => "html",
            Self::Markdown => "markdown", Self::Source => "source",
            Self::Logs => "logs", Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodecFamily {
    FormatCondenser, YamlCondenser, Lines, Dictionary,
}

impl CodecFamily {
    pub const ALL: [Self; 4] = [
        Self::FormatCondenser, Self::YamlCondenser, Self::Lines, Self::Dictionary,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::FormatCondenser => "format-condenser",
            Self::YamlCondenser => "yaml-condenser",
            Self::Lines => "lines",
            Self::Dictionary => "dict",
        }
    }

    fn bit(self) -> usize {
        match self {
            Self::FormatCondenser => 0, Self::YamlCondenser => 1,
            Self::Lines => 2, Self::Dictionary => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodecPolicy {
    pub schema_version: u16,
    pub file_kind: FileKind,
    pub enabled_codecs: Vec<CodecFamily>,
    pub allow_composition: bool,
    pub max_candidates: usize,
    pub latency_weight: f64,
}

impl CodecPolicy {
    pub fn from_genome(file_kind: FileKind, genome: &[bool]) -> Option<Self> {
        if genome.len() != POLICY_GENOME_BITS {
            return None;
        }
        let enabled_codecs = CodecFamily::ALL.into_iter()
            .filter(|codec| genome[codec.bit()])
            .collect();
        let max_candidates = match (genome[6], genome[7]) {
            (false, false) => 4, (false, true) => 8,
            (true, false) => 16, (true, true) => 32,
        };
        let latency_weight = if genome[8] { 0.25 } else { 0.0 };
        Some(Self {
            schema_version: POLICY_SCHEMA_VERSION,
            file_kind,
            enabled_codecs,
            allow_composition: genome[4],
            max_candidates,
            latency_weight,
        })
    }

    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.schema_version != POLICY_SCHEMA_VERSION {
            return Err(PolicyError::UnsupportedSchema(self.schema_version));
        }
        if ![4, 8, 16, 32].contains(&self.max_candidates) {
            return Err(PolicyError::InvalidCandidateCap(self.max_candidates));
        }
        if ![0.0, 0.1, 0.25, 0.5].contains(&self.latency_weight) {
            return Err(PolicyError::InvalidLatencyWeight);
        }
        if self.enabled_codecs.is_empty() && !self.allow_composition {
            return Err(PolicyError::NoCandidateFamilies);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyError {
    UnsupportedSchema(u16),
    InvalidCandidateCap(usize),
    InvalidLatencyWeight,
    NoCandidateFamilies,
}

/// Measurements from executing a policy against one corpus sample. Output tokens must include
/// all wire/header overhead and be measured with the deployment tokenizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitnessMeasurement {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_micros: u64,
    pub reconstruction_valid: bool,
}

impl FitnessMeasurement {
    /// Higher is better: token savings ratio minus a bounded, weighted latency ratio.
    /// Invalid reconstruction is a hard rejection rather than a soft penalty.
    pub fn score(self, latency_weight: f64) -> f64 {
        if !self.reconstruction_valid || self.input_tokens == 0 || !latency_weight.is_finite() {
            return f64::NEG_INFINITY;
        }
        let token_savings = (self.input_tokens as f64 - self.output_tokens as f64)
            / self.input_tokens as f64;
        let latency_scale = self.input_tokens.max(1) as f64 * 1_000.0;
        let latency_penalty = (self.latency_micros as f64 / latency_scale).min(10.0);
        token_savings - latency_weight * latency_penalty
    }
}

/// Tune a policy for one file kind. The callback must execute the policy on a fixed training corpus
/// and return actual tokenizer and latency measurements.
pub fn tune_policy<F>(
    file_kind: FileKind,
    config: BinarySwarmConfig,
    mut evaluate: F,
) -> Option<(CodecPolicy, BinarySwarmResult)>
where
    F: FnMut(&CodecPolicy) -> FitnessMeasurement,
{
    let swarm = BinarySwarm { config };
    let result = swarm.optimize(POLICY_GENOME_BITS, |genome| {
        let Some(policy) = CodecPolicy::from_genome(file_kind, genome) else {
            return f64::NEG_INFINITY;
        };
        if policy.validate().is_err() {
            return f64::NEG_INFINITY;
        }
        evaluate(&policy).score(policy.latency_weight)
    })?;
    let policy = CodecPolicy::from_genome(file_kind, &result.position)?;
    policy.validate().ok()?;
    Some((policy, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_versioned_policy_for_a_file_kind() {
        let mut genome = vec![false; POLICY_GENOME_BITS];
        genome[0] = true;
        genome[2] = true;
        genome[4] = true;
        genome[7] = true;
        genome[8] = true;
        let policy = CodecPolicy::from_genome(FileKind::Json, &genome).unwrap();
        assert_eq!(policy.schema_version, POLICY_SCHEMA_VERSION);
        assert_eq!(policy.file_kind, FileKind::Json);
        assert_eq!(policy.enabled_codecs, vec![CodecFamily::FormatCondenser, CodecFamily::Lines]);
        assert!(policy.allow_composition);
        assert_eq!(policy.max_candidates, 8);
        assert_eq!(policy.latency_weight, 0.25);
        assert_eq!(policy.validate(), Ok(()));
    }

    #[test]
    fn rejects_unsupported_schema_and_invalid_policy() {
        let mut genome = vec![false; POLICY_GENOME_BITS];
        genome[6] = true;
        genome[7] = true;
        let mut policy = CodecPolicy::from_genome(FileKind::Csv, &genome).unwrap();
        assert_eq!(policy.validate(), Err(PolicyError::NoCandidateFamilies));
        policy.schema_version += 1;
        assert_eq!(policy.validate(), Err(PolicyError::UnsupportedSchema(2)));
        assert!(CodecPolicy::from_genome(FileKind::Csv, &[false]).is_none());
    }

    #[test]
    fn invalid_round_trip_is_never_a_usable_fitness() {
        let measurement = FitnessMeasurement {
            input_tokens: 100, output_tokens: 10, latency_micros: 1,
            reconstruction_valid: false,
        };
        assert_eq!(measurement.score(0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn fitness_rewards_token_savings_and_penalizes_latency() {
        let fast = FitnessMeasurement {
            input_tokens: 100, output_tokens: 60, latency_micros: 1_000,
            reconstruction_valid: true,
        };
        let slow = FitnessMeasurement { latency_micros: 100_000, ..fast };
        assert!(fast.score(0.25) > slow.score(0.25));
        assert!(fast.score(0.0) > 0.0);
    }

    #[test]
    fn tuner_returns_a_valid_policy_and_uses_measurements() {
        let (policy, result) = tune_policy(
            FileKind::Json,
            BinarySwarmConfig { particles: 16, iterations: 12, seed: 42, ..Default::default() },
            |policy| FitnessMeasurement {
                input_tokens: 100,
                output_tokens: if policy.enabled_codecs.contains(&CodecFamily::FormatCondenser) {
                    30
                } else { 80 },
                latency_micros: 500,
                reconstruction_valid: !policy.enabled_codecs.is_empty(),
            },
        ).unwrap();
        assert_eq!(policy.schema_version, POLICY_SCHEMA_VERSION);
        assert_eq!(policy.file_kind, FileKind::Json);
        assert!(policy.validate().is_ok());
        assert!(result.fitness.is_finite());
    }
}
