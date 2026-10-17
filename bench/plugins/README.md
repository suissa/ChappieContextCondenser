# Plugin isolation and genetic benchmark

Run from the repository root:

    cargo test -p secondwind-optimize --test plugin_genetic_bench -- --nocapture

The integration benchmark tests the five existing structured Transform plugins (columnar, norm, nested, doc, variables-simplifier) plus the YAML text proposer. It includes a baseline and 31 non-empty plugin portfolios for each fixture family. A genome is a bit mask selecting which plugins are allowed to compete in the optimizer's best-of-N style; it does not chain incompatible wire formats together.

Each file family receives an independent fitness run. The seeded genetic algorithm uses 32 individuals (greater than the 13 families in the corpus), 24 generations, elite retention, crossover, and mutation. Fitness prioritizes absolute byte reduction, with a small penalty for enabling unnecessary plugins. Invalid JSON round trips and expansions are not rewarded. The benchmark also exhaustively evaluates all 32 masks, so the GA result can be compared against the known best portfolio for the same fixture.

The fixture collection covers high-/low-cardinality JSON arrays, flat JSON objects, small JSON arrays, YAML, TOML, Markdown, CSV, NDJSON, logs, diffs, source code, and XML. Unsupported formats are expected to select the baseline; the report makes this visible rather than claiming universal compression.

Important scope limits:
- This is a deterministic unit-style benchmark over checked-in fixtures, not a production traffic result.
- Current Transform plugins operate on structured JSON. YAML is a separate TextProposer-style codec with a semantic YAML contract.
- The benchmark compares byte counts. Use the existing tokenizer benchmarks for token-level claims.
- Genetic optimization here is per file family, not one pooled score across file types. The exhaustive search is a validation oracle for the GA.
- Existing offload, relevance selection, deduplication, cache-stability, prose shrinking, proof/admission, and detector gates remain separate mechanisms in the production pipeline; this benchmark does not claim to benchmark all of them as standalone codecs.

## Swarm refinement after the genetic search

After the genetic algorithm produces a candidate for one file family, the benchmark runs Binary Particle Swarm Optimization (BPSO) over the same plugin-activation bit vector. The swarm starts with the GA winner, its one-bit neighborhood, and seeded random particles. Each particle tracks its own best position and the global best. Continuous velocities are updated with inertia, cognitive attraction, and social attraction; a sigmoid maps each velocity to a probability of enabling a codec. The BPSO stage is deterministic under the benchmark's seeded PRNG and is not allowed to regress below the GA candidate's fitness.

Parameters in this benchmark: 24 particles, 32 iterations, inertia 0.62, cognitive/social coefficients 1.45. They are starting values for experimentation, not empirically established optima.

Why BPSO: the genome is naturally a binary vector of plugin choices, and particle updates balance exploration with movement toward good candidates. The GA supplies diversity and recombination; BPSO adds a different search dynamic around promising candidates. We retain exhaustive enumeration as an oracle and report GA, swarm, and exhaustive masks so heuristic search quality remains measurable.

Important caveat: with five binary plugin switches there are only 32 portfolios, so exhaustive enumeration is cheap and gives the exact answer. BPSO is useful as the second-stage architecture when the genome expands to per-plugin parameters (thresholds, minimum input size, alias limits, row caps, offload policy, tokenizer objective, latency budget) and the search space becomes too large for exhaustive enumeration. The current benchmark optimizes plugin eligibility, not all of those parameters yet.
