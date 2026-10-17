# VariablesSimplifier

`VariablesSimplifier` is an additional structured codec in the optimizer's best-of-N search. It replaces JSON object keys with deterministic aliases of at most three ASCII characters and emits a dictionary that maps each alias back to the original key.

## Wire format

```text
SWVAR1
{"a":"customerIdentifier","b":"customerDisplayName","c":"status"}
[{"a":"C123","b":"Ana","c":"active"},{"a":"C456","b":"Bia","c":"inactive"}]
```

The dictionary is emitted once per block, so repeated keys across records and nested objects can be represented more compactly. Aliases are deterministic: distinct original keys are sorted before assigning short codes. String values, numbers, booleans, nulls, arrays, and object nesting are preserved.

## Safety and admission

- The codec reconstructs the original JSON value before returning a candidate.
- The optimizer's normal admission gate compares canonical values and leaf-multiset hashes, and verifies the codec's round trip.
- The optimizer's net-cost gate decides whether the wire is actually cheaper. If the dictionary overhead means the candidate is not a real saving, it is not shipped.
- Duplicate JSON keys are still rejected by the existing input guard.
- Unknown aliases, malformed dictionaries, and malformed payloads fail decoding instead of guessing.

Use `secondwind_optimize::variables_simplifier::decode(wire)` to decode a wire produced by this codec. The codec is not a source-code variable renamer: it only rewrites JSON object keys. It does not alter arbitrary text, executable code, API schemas, or model responses. A proxy response-path restoration mechanism is not implied by this codec; response bytes remain unchanged, consistent with the proxy's existing contract.

## Validation

Unit tests cover nested/repeated keys, exact round trips, deterministic aliases, alias length, unchanged string values, and malformed/unknown aliases. Run:

```sh
cargo test -p secondwind-optimize variables_simplifier
cargo test -p secondwind-optimize
```
