# Genetic candidates and BPSO refinement

This stage adds a deterministic binary particle-swarm optimizer to the Rust optimization crate.
It is a reusable search primitive for the planned per-file-type policy tuner; it does not silently
change production codec selection. The existing lossless verification and token-cost gates remain
authoritative.

## Search pipeline

1. **Candidate generation (GA or exhaustive enumeration):** produce valid codec policies for a
   file type. A genome is a fixed-length bit vector; each bit represents one discrete decision,
   such as enabling a codec, allowing a codec composition, or enabling an optional preprocessing
   step.
2. **Proof gate:** reject any candidate that cannot reconstruct the original input exactly (or pass
   the format-specific semantic equivalence check).
3. **Fitness measurement:** score only proved candidates using measured token counts, latency, and
   any operational penalties. Do not use byte length as a proxy when the deployment has a tokenizer.
4. **BPSO refinement:** seed a swarm around the discrete search space and optimize the fitness.
5. **Independent validation:** replay the selected policy against held-out files for the same file
   type. Keep the prior GA/exhaustive winner if the swarm candidate regresses on the validation set.

## Defaults and guarantees

BinarySwarm::default() uses 24 particles and 32 iterations. It has a fixed seed, so the same
objective and configuration produce reproducible results. Duplicate genomes are memoized within a
run, and fitness_evaluations reports unique evaluations. The global best is elitist: the returned
candidate is the best observed, and best_fitness_by_iteration is monotonic.

The engine maximizes fitness. Non-finite objective values are treated as invalid (negative infinity).
A caller must still enforce its own hard constraints before returning a score. The swarm is a
heuristic, not a proof of global optimality; use exhaustive search as the reference when the genome
is small enough.

## API sketch

```rust
use secondwind_optimize::swarm::BinarySwarm;

let result = BinarySwarm::default()
    .optimize(genome_width, |genome| {
        // Decode the bits into a per-format policy.
        // Reject candidates that fail the round-trip proof.
        // Measure actual tokenizer cost and latency on this format's corpus.
        measured_fitness(genome)
    })
    .expect("genome width and particle count must be nonzero");

println!("fitness: {}", result.fitness);
println!("policy bits: {:?}", result.position);
println!("unique evaluations: {}", result.fitness_evaluations);
```

measured_fitness is application-specific: the library deliberately does not invent a universal
trade-off between token reduction, latency, memory, and CPU. The next integration step is to define a
versioned per-file-type genome and connect its decoder to the existing proposer registry, then run
GA, BPSO, exhaustive baselines for small genomes, and held-out corpus validation in CI.
