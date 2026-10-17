# Format condensers

The optimizer treats format-specific condensation as a competition, not as a mandatory rewrite. A candidate is emitted only when it is smaller under the configured token counter and passes its format's reconstruction proof.

| Target | Strategy | Proof | Wire | Notes |
|---|---|---|---|---|
| JSON | minify + recursive object-key dictionary | parsed `serde_json::Value` equality | `SJSON1` | values are never dictionary-rewritten; duplicate-key JSON remains protected by the existing admission path |
| YAML | comment removal + explicit next-line indentation + key dictionary | parsed `serde_yaml::Value` equality | `SWY1` | block scalars are declined; `#` inside quoted strings remains data |
| TOML | remove blank/comment-only lines | parsed `toml::Value` equality | `STOML1` | inline comments remain untouched |
| XML | not yet enabled | — | — | requires an XML data-model policy because whitespace-only text nodes can be significant |
| CSV | not yet enabled | — | — | requires RFC 4180-aware parsing plus a schema-preserving wire format |
| Markdown | not yet enabled | — | — | whitespace, code fences, tables and hard breaks can be presentation or content |
| source code | existing Tree-sitter path for Rust/Python when the feature is enabled | parser/tree proof | existing tree codecs | identifier rewriting remains a separate, explicit transformation |
| arbitrary text/logs | existing grouped/tree/lock/prose/offload strategies | codec-specific or content-covering proof | existing formats | no unsafe comment stripping is applied generically |

## Why the unsupported formats are not approximated

The objective is not to make the file look smaller. The objective is to reduce model tokens while keeping the information recoverable. For XML, CSV and Markdown, a naive whitespace/comment pass can silently change data. The implementation therefore refuses a candidate until a parser-backed semantic proof and a reversible response path exist.

The intended next layer is parser-backed codecs for XML, CSV and Markdown, followed by corpus-driven optimization. Each codec should expose `encode`, `decode`, and `verify`, then compete through the same net-cost gate as SWY1 and the JSON codecs.

## Maximum-space policy

For every supported format, the optimizer should search at least these candidates:

1. canonical/minified serialization;
2. repeated-key/name dictionary encoding;
3. structural indentation or delimiter coding;
4. repeated-value/prefix dictionary encoding when the format permits it;
5. combinations of the above;
6. original representation as the fallback.

The cheapest candidate after proof and detector checks wins. No fixed compression percentage is assumed. Small documents frequently lose because the dictionary/header overhead dominates; large repetitive tool results are where the structural codecs are expected to win.
