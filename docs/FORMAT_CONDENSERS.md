# Format condensers

The optimizer treats format-specific condensation as a competition, not as a mandatory rewrite. A candidate is emitted only when it is smaller under the configured token counter and passes its format's reconstruction proof.

| Target | Strategies searched | Proof / boundary |
|---|---|---|
| JSON | Existing structured transforms; compact JSON + key dictionary; repeated-line and repeated-token dictionaries; semantic-wire compositions and stacked line/token dictionaries | Parsed JSON equality for semantic codec; exact byte round-trip for dictionaries. Duplicate-key JSON stays protected by the existing path. |
| YAML | SWY1 structural/key dictionary; repeated-line and repeated-token dictionaries; compositions over semantic SWY1 | Parsed YAML equality for SWY1; exact byte round-trip for generic dictionaries. Block scalars are declined by SWY1. |
| TOML | Comment/blank-line compaction; repeated-line and repeated-token dictionaries; semantic-wire compositions | Parsed TOML equality for STOML1; exact byte round-trip for generic dictionaries. |
| XML / HTML | Repeated-line dictionary, repeated-token dictionary, and composition with a semantic candidate when applicable | Exact byte round-trip only. No whitespace or comment stripping: whitespace can be meaningful in text nodes, scripts, styles, and preformatted content. |
| CSV | Repeated-line and repeated-token dictionaries | Exact byte round-trip. No delimiter normalization or quote rewriting without an RFC-aware parser and explicit dialect. |
| Markdown | Repeated-line and repeated-token dictionaries | Exact byte round-trip. No whitespace/comment stripping that could change code fences, hard breaks, tables, or links. |
| Source code | Repeated-line/repeated-token dictionaries plus existing Tree-sitter-related paths where enabled | Exact byte round-trip for dictionary codecs. No identifier renaming or syntax rewriting in this matrix. |
| Logs / arbitrary text | Grouped/tree/lock codecs, repeated-line dictionary, repeated-token dictionary, and offload fallback | Exact byte round-trip for dictionary codecs; shared detector and cost gates. |

The matrix includes each codec as a separate proposer and also asks `FormatMatrix` for multiple candidates, including both orders of line/token composition and two-stage stacks over semantic JSON/TOML/YAML wires. The optimizer evaluates every candidate with its configured token counter, checks the codec's `verify` contract, applies detector checks, and then selects the lowest-cost profitable wire.
## Safety boundary

The generic line and token dictionaries are lossless reversible encodings, not format parsers. They can operate on CSV, XML, HTML, Markdown, source code, logs, and unknown text without assuming that whitespace or punctuation is disposable. Format-aware semantic rewrites are limited to formats whose parsed values can be compared.

A candidate is not accepted merely because it is shorter. It must pass its decoder/semantic verifier, the detector gate, and the optimizer's actual token-cost gate. Small documents can remain verbatim because headers and dictionaries cost more than they save.

## Maximum-space policy

For each input, the optimizer now searches available standalone and composed candidates. The strategy family includes:

1. canonical/minified serialization;
2. repeated-key/name dictionary encoding;
3. structural indentation or delimiter coding;
4. repeated-value/prefix dictionary encoding when the format permits it;
5. combinations of the above;
6. original representation as the fallback.

The cheapest candidate after proof and detector checks wins. No fixed compression percentage is assumed. Small documents frequently lose because the dictionary/header overhead dominates; large repetitive tool results are where the structural codecs are expected to win.
