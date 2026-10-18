//! Deterministic binary particle-swarm search for tuning discrete optimizer policies.
//!
//! Fitness is maximized. Callers should include token savings and any latency/quality penalties
//! in their objective, and should reject candidates that fail lossless reconstruction before scoring.

use std::collections::BTreeMap;

/// Configuration for binary particle-swarm optimization (BPSO).
#[derive(Clone, Copy, Debug)]
pub struct BinarySwarmConfig {
    /// Number of candidate policies maintained simultaneously.
    pub particles: usize,
    /// Number of position/velocity updates after initialization.
    pub iterations: usize,
    /// Deterministic PRNG seed; equal inputs produce equal results.
    pub seed: u64,
    /// Velocity persistence.
    pub inertia: f64,
    /// Attraction toward each particle's personal best.
    pub cognitive: f64,
    /// Attraction toward the swarm's global best.
    pub social: f64,
    /// Absolute velocity bound, preventing sigmoid saturation from becoming permanent.
    pub max_velocity: f64,
}

impl Default for BinarySwarmConfig {
    fn default() -> Self {
        Self {
            particles: 24,
            iterations: 32,
            seed: 0xB5_50_2026,
            inertia: 0.729,
            cognitive: 1.49445,
            social: 1.49445,
            max_velocity: 6.0,
        }
    }
}

/// Best discrete candidate found by the swarm.
#[derive(Clone, Debug, PartialEq)]
pub struct BinarySwarmResult {
    /// One bit per decision in the policy genome.
    pub position: Vec<bool>,
    /// Best objective value; higher is better.
    pub fitness: f64,
    /// Number of unique fitness evaluations (cached repeats are excluded).
    pub fitness_evaluations: usize,
    /// Global-best fitness after initialization and after every iteration.
    pub best_fitness_by_iteration: Vec<f64>,
}

/// Binary particle swarm optimizer. No RNG dependency or global state is used.
#[derive(Clone, Copy, Debug)]
pub struct BinarySwarm {
    pub config: BinarySwarmConfig,
}

impl Default for BinarySwarm {
    fn default() -> Self {
        Self { config: BinarySwarmConfig::default() }
    }
}

impl BinarySwarm {
    /// Search a bit-vector space and return the best candidate observed.
    ///
    /// The fitness callback must be deterministic for a given position. Non-finite scores are
    /// treated as invalid candidates (negative infinity). Zero dimensions or zero particles return None.
    pub fn optimize<F>(&self, dimensions: usize, mut fitness: F) -> Option<BinarySwarmResult>
    where
        F: FnMut(&[bool]) -> f64,
    {
        let cfg = self.config;
        if dimensions == 0 || cfg.particles == 0 {
            return None;
        }

        let mut rng = DeterministicRng::new(cfg.seed);
        let mut cache: BTreeMap<Vec<bool>, f64> = BTreeMap::new();
        let mut evaluations = 0usize;
        let mut positions: Vec<Vec<bool>> = (0..cfg.particles)
            .map(|_| (0..dimensions).map(|_| rng.unit() >= 0.5).collect())
            .collect();
        let mut velocities: Vec<Vec<f64>> = (0..cfg.particles)
            .map(|_| (0..dimensions).map(|_| rng.unit() * 2.0 - 1.0).collect())
            .collect();

        let mut personal_best = positions.clone();
        let mut personal_fitness = Vec::with_capacity(cfg.particles);
        for position in &positions {
            personal_fitness.push(score(position, &mut fitness, &mut cache, &mut evaluations));
        }

        let mut best_index = best_index(&personal_fitness);
        let mut global_best = personal_best[best_index].clone();
        let mut global_fitness = personal_fitness[best_index];
        let mut history = vec![global_fitness];

        for _ in 0..cfg.iterations {
            for particle in 0..cfg.particles {
                for dimension in 0..dimensions {
                    let current = if positions[particle][dimension] { 1.0 } else { 0.0 };
                    let personal = if personal_best[particle][dimension] { 1.0 } else { 0.0 };
                    let global = if global_best[dimension] { 1.0 } else { 0.0 };
                    let velocity = cfg.inertia * velocities[particle][dimension]
                        + cfg.cognitive * rng.unit() * (personal - current)
                        + cfg.social * rng.unit() * (global - current);
                    let velocity = velocity.clamp(-cfg.max_velocity.abs(), cfg.max_velocity.abs());
                    velocities[particle][dimension] = velocity;
                    let probability = 1.0 / (1.0 + (-velocity).exp());
                    positions[particle][dimension] = rng.unit() < probability;
                }

                let candidate_fitness = score(
                    &positions[particle],
                    &mut fitness,
                    &mut cache,
                    &mut evaluations,
                );
                if candidate_fitness > personal_fitness[particle] {
                    personal_fitness[particle] = candidate_fitness;
                    personal_best[particle] = positions[particle].clone();
                }
            }

            best_index = best_index(&personal_fitness);
            if personal_fitness[best_index] > global_fitness {
                global_fitness = personal_fitness[best_index];
                global_best = personal_best[best_index].clone();
            }
            // Elitism: history is monotonic and the best candidate can never regress.
            history.push(global_fitness);
        }

        Some(BinarySwarmResult {
            position: global_best,
            fitness: global_fitness,
            fitness_evaluations: evaluations,
            best_fitness_by_iteration: history,
        })
    }
}

fn score<F>(
    position: &[bool],
    fitness: &mut F,
    cache: &mut BTreeMap<Vec<bool>, f64>,
    evaluations: &mut usize,
) -> f64
where
    F: FnMut(&[bool]) -> f64,
{
    if let Some(value) = cache.get(position) {
        return *value;
    }
    let value = fitness(position);
    let value = if value.is_finite() { value } else { f64::NEG_INFINITY };
    cache.insert(position.to_vec(), value);
    *evaluations += 1;
    value
}

fn best_index(scores: &[f64]) -> usize {
    let mut best = 0;
    for index in 1..scores.len() {
        if scores[index] > scores[best] {
            best = index;
        }
    }
    best
}

/// Small fixed-seed xorshift generator; output is stable across platforms.
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_high_fitness_binary_policy_and_keeps_elite_history() {
        let target = [true, false, true, true, false, true, false, true];
        let result = BinarySwarm::default()
            .optimize(target.len(), |bits| {
                bits.iter().zip(target).filter(|(actual, expected)| **actual == *expected).count()
                    as f64
            })
            .unwrap();

        assert_eq!(result.fitness, target.len() as f64);
        assert_eq!(result.position, target);
        assert_eq!(result.best_fitness_by_iteration.len(), 33);
        assert!(result.best_fitness_by_iteration.windows(2).all(|w| w[1] >= w[0]));
        assert!(result.fitness_evaluations <= 24 * 33);
    }

    #[test]
    fn fixed_seed_produces_repeatable_results() {
        let swarm = BinarySwarm::default();
        let objective = |bits: &[bool]| bits.iter().filter(|bit| **bit).count() as f64;
        assert_eq!(
            swarm.optimize(16, objective).unwrap(),
            swarm.optimize(16, objective).unwrap()
        );
    }

    #[test]
    fn caches_duplicate_genomes_and_rejects_invalid_dimensions() {
        let swarm = BinarySwarm {
            config: BinarySwarmConfig { particles: 8, iterations: 4, seed: 1, ..Default::default() },
        };
        let mut calls = 0usize;
        let result = swarm.optimize(1, |_| { calls += 1; 1.0 }).unwrap();
        assert_eq!(calls, result.fitness_evaluations);
        assert!(result.fitness_evaluations <= 2);
        assert!(swarm.optimize(0, |_| 0.0).is_none());
    }

    #[test]
    fn non_finite_fitness_is_treated_as_invalid() {
        let result = BinarySwarm::default().optimize(3, |_| f64::NAN).unwrap();
        assert_eq!(result.fitness, f64::NEG_INFINITY);
    }
}
