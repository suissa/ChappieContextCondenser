# Versioned codec policy tuner

Schema version: 1. This stage defines a per-file-kind genome and fitness contract that can be
optimized by the existing deterministic BPSO implementation. It intentionally does not change the
production compressor's codec registry until a caller supplies a corpus-backed evaluator.

## Genome (9 bits)

| Bits | Meaning | Values |
|---|---|---|
| 0 | FormatCondenser | disabled / enabled |
| 1 | YamlCondenser | disabled / enabled |
| 2 | Lines | disabled / enabled |
| 3 | Dictionary | disabled / enabled |
| 4 | Composition | disabled / enabled |
| 5 | Reserved in schema v1 | false / true; currently no policy effect |
| 6–7 | Candidate cap | 4, 8, 16, 32 |
| 8 | Latency penalty | 0.0 or 0.25 |

A policy also records a file kind (JSON, YAML, TOML, CSV, XML, HTML, Markdown, source, logs, or
unknown). Policies are validated against the schema version before use. Empty codec sets without
composition are invalid and receive negative-infinity fitness.

## Fitness contract

The evaluator must run the policy against a fixed training corpus and return:

- input tokens and output tokens measured with the deployment tokenizer, including all wire headers;
- wall-clock latency in microseconds;
- a boolean indicating exact reconstruction or the approved format-specific semantic equivalence.

Fitness is token savings ratio minus a bounded, weighted latency ratio. A failed reconstruction,
zero-token input, or non-finite latency weight is rejected. This contract is intentionally explicit:
a host can replace the default score with a domain-specific objective later, but it must never reward
an invalid transform.

## API

The public entry point is `policy::tune_policy(file_kind, config, evaluator)`. The evaluator is
called for valid decoded policies; it should measure actual results rather than infer compression
from the policy bits. The returned policy is the best genome observed by BPSO, not a guarantee of a
global optimum.

## Next integration

The next change should provide a production evaluator that dispatches the enabled codec families
to their implementations, applies the existing proof/detector/token gates, measures latency and
token cost, and persists the selected policy by file kind and schema version. A held-out corpus must
validate any tuned policy before it can become a default.
