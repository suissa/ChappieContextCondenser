# YAML Structural Condenser (SWY1)

## Purpose

`YamlCondenser` is a model-facing representation for reducing the token cost of YAML configuration files. It removes presentation-only material, encodes indentation explicitly, and substitutes repeated mapping keys with short aliases. The wire format is named **SWY1** by this project.

It is not a replacement YAML syntax, and SWY1 must not be passed to a YAML parser or deployment tool. It is an intermediate protocol intended for a model-facing compression boundary.

## Is this an existing standard or algorithm?

There is no known YAML standard that defines this exact combination of a `SWY1` header, a key dictionary, and a trailing `|N` next-line-indentation marker. **SWY1 is a project-specific format**, assembled from established ideas:

- **Dictionary encoding / symbol substitution:** repeated mapping keys are replaced by short aliases and a dictionary maps each alias back to its original key.
- **Structural or syntax-aware compression:** the transformation operates on YAML-like structure rather than applying a generic byte compressor to the entire file.
- **Whitespace / indentation coding:** the indentation of the next non-comment line is represented as a number instead of repeating spaces.
- **Canonical semantic comparison:** the source and decoded output are parsed into YAML data models and compared for equality.
- **Reversible transformation / round-trip validation:** a candidate is accepted only if decoding reproduces an equivalent YAML data model.

It is conceptually related to dictionary-based compression and syntax-aware serialization, but it is **not ZIP/DEFLATE, LZ77, LZSS, or a standardized YAML compact form**. The aliases are not a hash and do not provide cryptographic integrity. The current YAML-specific proof is semantic equality through the parser, not a cryptographic certificate.

## Wire format

The current wire has three parts:

```text
SWY1
<JSON dictionary: short alias -> original mapping key>
<compact body; each line ends with |N>
```

Example input:

```yaml
# Runtime configuration
services:
  app:
    environment:
      NODE_ENV: production # deployment mode
      DATABASE_HOST: db
      DATABASE_PORT: 5432
```

Illustrative SWY1 output (aliases and exact spacing depend on the implementation):

```text
SWY1
{"a":"DATABASE_HOST","b":"DATABASE_PORT","c":"NODE_ENV","d":"environment","e":"app","f":"services"}
f:|2
e:|4
d:|6
c: production|6
a: db|6
b: 5432|0
```

The line suffix `|N` means: **the next encoded line starts with N spaces**. It is part of the SWY1 framing, not YAML's block-scalar syntax. The last line uses `|0` in this example. The dictionary is JSON so aliases can be decoded without guessing. Mapping-key aliases are deterministic and up to three characters, using a base-62 alphabet; aliases that collide with original keys are skipped.

## Encoding algorithm

1. Parse the source with the YAML parser. Invalid YAML is declined.
2. Scan source lines, remove comments recognized outside quoted strings, and omit blank/comment-only lines.
3. Collect eligible mapping keys and assign deterministic short aliases.
4. Replace eligible mapping keys with their aliases.
5. Append `|N` to each line, where `N` is the indentation of the next retained line.
6. Serialize the alias dictionary as JSON and prepend the `SWY1` header.
7. Decode the candidate and parse both source and decoded output. Return the candidate only when the resulting YAML data models compare equal. The optimizer's outer net-cost gate should then ensure the representation is actually cheaper before selecting it.

## Decoding and response restoration

- `YamlCondenser::decode(wire)` reconstructs a readable YAML representation by restoring aliases and applying the recorded indentation.
- `YamlCondenser::verify(source, wire)` checks semantic YAML equality.
- `YamlCondenser::restore_response_keys(wire, response)` is an explicit helper for restoring known aliases in a YAML response. It is not an automatic guarantee that every model response is decoded or rewritten by the proxy.

The decoded YAML is semantically equivalent, but not byte-identical to the source: comments, blank lines, original indentation choices, quoting style, and other presentation choices may be lost. If the original file must be preserved byte-for-byte, keep the original separately and treat SWY1 only as a temporary model-facing view.

## Comment handling and strings

YAML comments are presentation details, not nodes in the YAML representation graph. The YAML 1.2.2 specification states that comments must not affect the serialization tree or representation graph. SWY1 therefore omits recognized comments from its model-facing body.

A `#` character inside a quoted scalar is data, not a comment. The scanner tracks single- and double-quoted regions so examples such as `AUTH_SECRET: "a#b"` retain the `#b` text. YAML lexical rules are more complex than a simple line scan, however; comment stripping must remain guarded by parsing and round-trip validation. Comments can contain useful human documentation, and comments that represent disabled configuration are not active YAML data: removing them means the model will not see that disabled configuration unless it is retained separately.

## Current limitations

- **Literal and folded block scalars** (`|` and `>`) are declined. Their content, newlines, chomping, and indentation are semantically significant.
- Quoted mapping keys are not currently aliased by the line-level key rewrite.
- This is not a byte-preserving editor and does not retain comments or formatting.
- YAML features and edge cases must be covered by tests against the parser. A line-oriented transformation is not a complete YAML lexer.
- `restore_response_keys` is an opt-in helper, not an automatic response-path integration.
- No token-saving percentage is guaranteed. The dictionary and markers have overhead; the existing optimizer should select SWY1 only when it passes semantic verification and the actual net-cost gate.

## Standards and references

- [YAML 1.2.2 specification](https://yaml.org/spec/1.2/): defines the YAML data model and syntax.
- [YAML 1.2.2, comments](https://github.com/yaml/yaml-spec/blob/1.2.2/spec.md): comments are presentation details and do not affect the serialization tree or representation graph.
- [YAML 1.2.2, block scalar styles](https://yaml.org/spec/1.2/): explains why literal/folded scalar content and indentation need special treatment.

The YAML specification supplies the semantic rules used for validation; it does **not** define SWY1. SWY1 is the project's own reversible structural-condensation protocol.
