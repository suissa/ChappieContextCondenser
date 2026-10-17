use std::collections::BTreeMap;

use serde_json::Value;

use crate::transform::TextProposer;

/// Format-aware text condensation for model-facing tool output.
///
/// The codec deliberately separates detection from reconstruction. It never claims semantic
/// equivalence for a format whose parser is unavailable; unsupported formats return `None` and
/// continue through the existing lossless/offload pipeline.
pub struct FormatCondenser;

impl TextProposer for FormatCondenser {
    fn id(&self) -> &'static str {
        "format-condenser"
    }

    fn encode(&self, raw: &str) -> Option<String> {
        if looks_like_json(raw) {
            return compact_json(raw);
        }
        if looks_like_toml(raw) {
            return compact_toml(raw);
        }
        None
    }

    fn decode(&self, wire: &str) -> Option<String> {
        if let Some(body) = wire.strip_prefix("SJSON1\n") {
            let mut parts = body.splitn(2, '\n');
            let dictionary: BTreeMap<String, String> = serde_json::from_str(parts.next()?).ok()?;
            let compact = parts.next()?;
            return Some(restore_json_aliases(compact, &dictionary));
        }
        if let Some(body) = wire.strip_prefix("STOML1\n") {
            return Some(body.to_string());
        }
        None
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        match wire.strip_prefix("SJSON1\n") {
            Some(_) => {
                let Some(decoded) = self.decode(wire) else { return false };
                let Ok(a) = serde_json::from_str::<Value>(raw) else { return false };
                let Ok(b) = serde_json::from_str::<Value>(&decoded) else { return false };
                a == b
            }
            _ if wire.starts_with("STOML1\n") => {
                // TOML semantic verification is delegated to the TOML parser when that optional
                // codec is enabled. Without it, refuse rather than pretending textual equality is
                // sufficient.
                false
            }
            _ => false,
        }
    }
}

fn looks_like_json(raw: &str) -> bool {
    let s = raw.trim_start();
    (s.starts_with('{') && s.ends_with('}')) || (s.starts_with('[') && s.ends_with(']'))
}

fn compact_json(raw: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let compact = serde_json::to_string(&value).ok()?;
    if compact.len() >= raw.len() {
        return None;
    }

    // JSON aliases are intentionally applied only to object keys. Values remain untouched by the
    // dictionary layer, avoiding accidental rewriting of URLs, identifiers, hashes, or prose.
    let mut keys = value
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .filter(|k| k.len() > 3)
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();

    let mut dictionary = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() {
        dictionary.insert(alias(i), key.clone());
    }

    // For the first implementation, only use the compact JSON when the alias dictionary itself
    // has a measurable net win. Nested objects are handled by replacing quoted key tokens in the
    // already parsed compact JSON; values are never changed.
    let mut aliased = compact.clone();
    for (a, key) in &dictionary {
        let from = format!("\"{key}\":");
        let to = format!("\"{a}\":");
        aliased = aliased.replace(&from, &to);
    }
    let dict_json = serde_json::to_string(&dictionary).ok()?;
    let wire = format!("SJSON1\n{dict_json}\n{aliased}");
    (wire.len() < raw.len()).then_some(wire)
}

fn restore_json_aliases(compact: &str, dictionary: &BTreeMap<String, String>) -> String {
    let mut out = compact.to_string();
    for (alias, key) in dictionary {
        let from = format!("\"{alias}\":");
        let to = format!("\"{key}\":");
        out = out.replace(&from, &to);
    }
    out
}

fn looks_like_toml(raw: &str) -> bool {
    raw.lines().any(|line| {
        let s = line.trim();
        !s.is_empty() && !s.starts_with('#') && (s.contains('=') || (s.starts_with('[') && s.ends_with(']')))
    })
}

fn compact_toml(raw: &str) -> Option<String> {
    // TOML has no indentation significance. Removing complete comment lines is safe, but inline
    // comment stripping is intentionally deferred until a TOML parser is part of the codec.
    let mut out = String::new();
    for line in raw.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    if out.len() >= raw.len() { None } else { Some(format!("STOML1\n{out}")) }
}

fn alias(mut n: usize) -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::new();
    loop {
        out.push(A[n % A.len()] as char);
        n /= A.len();
        if n == 0 { break; }
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_compaction_and_aliases_are_semantically_verified() {
        let raw = r#"{
  "customer_name": "Ana",
  "customer_address": "Rua A",
  "customer_status": "active",
  "items": [{"customer_name": "Bia", "customer_status": "pending"}]
}"#;
        let codec = FormatCondenser;
        let wire = codec.encode(raw).unwrap();
        assert!(wire.starts_with("SJSON1\n"));
        assert!(wire.len() < raw.len());
        assert!(codec.verify(raw, &wire));
    }

    #[test]
    fn json_strings_are_not_rewritten_as_keys() {
        let raw = r#"{"message":"customer_name: customer_name", "customer_name":"x"}"#;
        let codec = FormatCondenser;
        let wire = codec.encode(raw).unwrap();
        assert!(codec.verify(raw, &wire));
        let decoded = codec.decode(&wire).unwrap();
        assert_eq!(serde_json::from_str::<Value>(raw).unwrap(), serde_json::from_str::<Value>(&decoded).unwrap());
    }

    #[test]
    fn toml_comment_lines_are_removed_but_inline_comments_are_preserved() {
        let raw = "# header\n[server]\nhost = \"localhost\" # keep inline\nport = 8080\n";
        let codec = FormatCondenser;
        let wire = codec.encode(raw).unwrap();
        assert!(wire.starts_with("STOML1\n"));
        let decoded = codec.decode(&wire).unwrap();
        assert!(decoded.contains("host = \"localhost\" # keep inline"));
        assert!(!decoded.contains("# header"));
    }
}
