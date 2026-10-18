# Persisted codec policies

Schema v1 policies are stored independently by file kind:

```text
<root>/
  codec-policies/
    v1/
      json.json
      yaml.json
      markdown.json
```

Use `CodecPolicyStore::new(root)`, then `save(&policy)`, `load(FileKind::Json)`, or
`remove(FileKind::Json)`. The store does not choose a policy automatically and does not alter the
compressor's default behavior: callers explicitly load a policy and decide when to activate it.

Safety properties:

- Saving rejects policies that fail current schema validation.
- Writes use a temporary file in the destination directory followed by rename, avoiding partial
  JSON documents being observed by readers.
- Loading returns `Ok(None)` for a missing policy, but malformed JSON, unknown fields, unsupported
  schema versions, invalid settings, and file-kind mismatches are explicit errors.
- Schema version is part of the path, so a future schema can coexist without overwriting old data.
- A loaded policy must still be evaluated against a held-out corpus before being promoted to a
  production default.

This stage provides persistence and validation only. It does not automatically activate persisted
policies or claim that a particular policy improves token savings; that requires corpus-backed
measurement with the deployment tokenizer.
