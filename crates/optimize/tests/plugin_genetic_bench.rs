//! Deterministic plugin isolation + genetic portfolio benchmark.
//!
//! Run with:
//!   cargo test -p secondwind-optimize --test plugin_genetic_bench -- --nocapture
//!
//! Each file family gets its own fitness search. Genomes enable a portfolio of the existing
//! Transform plugins; enabled codecs compete on the same input (the production best-of-N model).
//! The test never counts a lossy output as a win and always includes the unmodified baseline.

use secondwind_optimize::{
    columnar::Columnar,
    doc::Doc,
    nested::Nested,
    norm::Normalize,
    transform::Transform,
    variables_simplifier::VariablesSimplifier,
    yaml_condenser::YamlCondenser,
};
use serde_json::Value;
use std::collections::BTreeMap;

type Factory = fn() -> Box<dyn Transform>;

const POPULATION: usize = 32; // Must exceed the number of file families below.
const GENERATIONS: usize = 24;
const MUTATION_PER_MILLE: u64 = 90;

#[derive(Clone, Debug)]
struct Sample {
    family: &'static str,
    raw: String,
    json: Option<Value>,
}

#[derive(Clone, Copy)]
struct Plugin {
    name: &'static str,
    factory: Factory,
}

#[derive(Clone, Copy, Debug)]
struct Genome {
    mask: u8,
}

#[derive(Clone, Debug)]
struct Score {
    saved_bytes: usize,
    output_bytes: usize,
    codec: &'static str,
    valid: bool,
}

fn plugins() -> [Plugin; 5] {
    [
        Plugin { name: "columnar", factory: || Box::new(Columnar::default()) },
        Plugin { name: "norm", factory: || Box::new(Normalize::default()) },
        Plugin { name: "nested", factory: || Box::new(Nested::default()) },
        Plugin { name: "doc", factory: || Box::new(Doc::default()) },
        Plugin { name: "variables-simplifier", factory: || Box::new(VariablesSimplifier) },
    ]
}

fn samples() -> Vec<Sample> {
    let mut out = Vec::new();
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/compression/corpus");
    for (family, file) in [
        ("json-high-cardinality-array", "01-highcard-array.json"),
        ("json-low-cardinality-array", "02-lowcard-array.json"),
        ("json-flat-object", "03-flat-object.json"),
        ("json-small-array", "04-small-array.json"),
    ] {
        let path = corpus.join(file);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read benchmark fixture {}: {e}", path.display()));
        let json = serde_json::from_str::<Value>(&raw).ok();
        out.push(Sample { family, raw, json });
    }

    // Representative commonly encountered context file families. JSON-only transforms should
    // explicitly pass these through rather than pretending they compressed them.
    for (family, raw) in [
        ("yaml", "services:\n  api:\n    image: example/api:1\n    ports:\n      - \"8080:80\"\n"),
        ("toml", "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n"),
        ("markdown", "# Release notes\n\n- Fixed cache behavior\n- Added a stable plugin interface\n"),
        ("csv", "id,name,status\n1,alpha,ready\n2,beta,ready\n3,gamma,pending\n"),
        ("ndjson", "{\"id\":1,\"status\":\"ready\"}\n{\"id\":2,\"status\":\"ready\"}\n"),
        ("log", "2026-10-17T09:00:00Z INFO worker=1 status=ready\n2026-10-17T09:00:01Z INFO worker=2 status=ready\n"),
        ("diff", "diff --git a/app.rs b/app.rs\n+fn ready() -> bool { true }\n"),
        ("source-code", "pub fn total(values: &[u64]) -> u64 { values.iter().sum() }\n"),
        ("xml", "<items><item id=\"1\">alpha</item><item id=\"2\">beta</item></items>\n"),
    ] {
        out.push(Sample { family, raw: raw.to_owned(), json: serde_json::from_str(raw).ok() });
    }
    out
}

fn score(sample: &Sample, mask: u8, plugins: &[Plugin; 5]) -> Score {
    let baseline = sample.raw.len();
    let mut best = Score {
        saved_bytes: 0,
        output_bytes: baseline,
        codec: "baseline",
        valid: true,
    };
    let Some(value) = &sample.json else { return best };

    for (index, plugin) in plugins.iter().enumerate() {
        if mask & (1 << index) == 0 { continue; }
        let candidate = (plugin.factory)();
        let Some(encoded) = candidate.try_encode(value) else { continue };
        // Existing transform API exposes the reconstructed value for admission. The benchmark
        // rejects any candidate that changes JSON data, and only rewards a strict size reduction.
        if encoded.decoded != *value { continue; }
        let bytes = encoded.wire.len();
        if bytes < best.output_bytes {
            best = Score {
                saved_bytes: baseline - bytes,
                output_bytes: bytes,
                codec: plugin.name,
                valid: true,
            };
        }
    }
    best
}

fn fitness(sample: &Sample, mask: u8, plugins: &[Plugin; 5]) -> i64 {
    let s = score(sample, mask, plugins);
    if !s.valid { return -1_000_000; }
    // Reduction is the primary objective; the small complexity penalty prefers simpler portfolios
    // only when savings tie. No file-family averages are used to hide weak individual performance.
    (s.saved_bytes as i64 * 100) - (mask.count_ones() as i64)
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

fn evolve_for(sample: &Sample, plugins: &[Plugin; 5], seed: &mut u64) -> Genome {
    let max_mask = (1u8 << plugins.len()) - 1;
    let mut population: Vec<Genome> = (0..POPULATION)
        .map(|_| Genome { mask: (next(seed) as u8) & max_mask })
        .collect();
    let mut best = Genome { mask: 0 };

    for _ in 0..GENERATIONS {
        population.sort_by_key(|g| std::cmp::Reverse(fitness(sample, g.mask, plugins)));
        if fitness(sample, population[0].mask, plugins) > fitness(sample, best.mask, plugins) {
            best = population[0];
        }
        let elite = (POPULATION / 4).max(2);
        let parents = population[..elite].to_vec();
        let mut next_generation = parents.clone();
        while next_generation.len() < POPULATION {
            let a = parents[(next(seed) as usize) % parents.len()];
            let b = parents[(next(seed) as usize) % parents.len()];
            let cut = 1 + (next(seed) as u8 % (plugins.len() as u8 - 1));
            let low = (1u8 << cut) - 1;
            let mut mask = (a.mask & low) | (b.mask & !low);
            if next(seed) % 1000 < MUTATION_PER_MILLE {
                let bit = next(seed) as u8 % plugins.len() as u8;
                mask ^= 1 << bit;
            }
            next_generation.push(Genome { mask: mask & max_mask });
        }
        population = next_generation;
    }

    population.sort_by_key(|g| std::cmp::Reverse(fitness(sample, g.mask, plugins)));
    if fitness(sample, population[0].mask, plugins) > fitness(sample, best.mask, plugins) {
        best = population[0];
    }
    best
}

#[test]
fn benchmark_each_plugin_alone_and_every_combination_per_file_family() {
    let plugins = plugins();
    let samples = samples();
    assert!(POPULATION > samples.len(), "population must exceed number of file families");
    let mut results = BTreeMap::new();
    let mut seed = 0x5EC0_2D1A_2026_u64;

    println!("family,baseline_bytes,best_codec,best_bytes,saved_bytes,reduction_pct,ga_mask,swarm_mask,exhaustive_mask");
    for sample in &samples {
        // Isolation pass: baseline, every plugin alone, and every plugin combination (2^N masks).
        let mut exhaustive = (0u8..(1u8 << plugins.len()))
            .map(|mask| (mask, score(sample, mask, &plugins)))
            .collect::<Vec<_>>();
        exhaustive.sort_by_key(|(_, s)| std::cmp::Reverse(s.saved_bytes));
        let (exhaustive_mask, exhaustive_score) = exhaustive[0].clone();

        // A distinct genetic run per file family, rather than one aggregate fitness across files.
        let ga = evolve_for(sample, &plugins, &mut seed);
        let swarm = swarm_refine(sample, &plugins, &mut seed, ga);
        let swarm_score = score(sample, swarm.mask, &plugins);
        let reduction = if sample.raw.is_empty() { 0.0 } else {
            100.0 * swarm_score.saved_bytes as f64 / sample.raw.len() as f64
        };
        println!("{},{},{},{},{},{:.2},{:#07b},{:#07b},{:#07b}",
            sample.family, sample.raw.len(), swarm_score.codec, swarm_score.output_bytes,
            swarm_score.saved_bytes, reduction, ga.mask, swarm.mask, exhaustive_mask);

        assert!(swarm_score.valid, "swarm returned invalid result for {}", sample.family);
        assert!(swarm_score.output_bytes <= sample.raw.len(), "fitness may not reward expansion");
        assert!(fitness(sample, swarm.mask, &plugins) >= fitness(sample, ga.mask, &plugins),
            "swarm must not regress from GA candidate for {}", sample.family);
        results.insert(sample.family, (swarm_score.saved_bytes, exhaustive_score.saved_bytes));
    }

    // YAML is also registered as a TextProposer-style plugin. It is checked independently because
    // its contract is semantic YAML equivalence, not JSON Transform::decoded equality.
    let yaml = samples.iter().find(|s| s.family == "yaml").unwrap();
    let yaml_wire = YamlCondenser::encode(&yaml.raw);
    println!("yaml-plugin,baseline_bytes,wire_bytes,saved_bytes");
    if let Some(wire) = yaml_wire {
        println!("yaml-plugin,{},{},{}", yaml.raw.len(), wire.len(), yaml.raw.len().saturating_sub(wire.len()));
    } else {
        println!("yaml-plugin,{},{},0", yaml.raw.len(), yaml.raw.len());
    }

    assert_eq!(results.len(), samples.len());
}
