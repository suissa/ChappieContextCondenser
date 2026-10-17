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
