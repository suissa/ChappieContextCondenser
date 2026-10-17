use std::collections::BTreeMap;

use serde_json::Value;

use crate::transform::TextProposer;

/// Format-aware text condensation for model-facing tool output.
///
/// The codec deliberately separates detection from reconstruction. It only emits a format when a
/// parser can prove the reconstructed document represents the same data model.
pub struct FormatCondenser;

impl TextProposer for FormatCondenser {
    fn id(&self) -> &'static str { "format-condenser" }

    fn encode(&self, raw: &str) -> Option<String> {
        if looks_like_json(raw) { return compact_json(raw); }
        if looks_like_toml(raw) { return compact_toml(raw); }
        None
    }

    fn decode(&self, wire: &str) -> Option<String> {
        if let Some(body) = wire.strip_prefix("SJSON1\n") {
            let mut parts = body.splitn(2, '\n');
            let dictionary: BTreeMap<String, String> = serde_json::from_str(parts.next()?).ok()?;
            return Some(restore_json_aliases(parts.next()?, &dictionary));
        }
        wire.strip_prefix("STOML1\n").map(str::to_owned)
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        if wire.starts_with("SJSON1\n") {
            let Some(decoded) = self.decode(wire) else { return false };
            let Ok(a) = serde_json::from_str::<Value>(raw) else { return false };
            let Ok(b) = serde_json::from_str::<Value>(&decoded) else { return false };
            return a == b;
        }
        if wire.starts_with("STOML1\n") {
            let Some(decoded) = self.decode(wire) else { return false };
            let Ok(a) = raw.parse::<toml::Value>() else { return false };
            let Ok(b) = decoded.parse::<toml::Value>() else { return false };
            return a == b;
        }
        false
    }
}

fn looks_like_json(raw: &str) -> bool {
    let s = raw.trim_start();
    (s.starts_with('{') && s.ends_with('}')) || (s.starts_with('[') && s.ends_with(']'))
}

fn collect_object_keys(value: &Value, keys: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key.len() > 3 { keys.push(key.clone()); }
                collect_object_keys(value, keys);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_object_keys(v, keys)),
        _ => {}
    }
}

fn compact_json(raw: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let compact = serde_json::to_string(&value).ok()?;
    if compact.len() >= raw.len() { return None; }

    let mut keys = Vec::new();
    collect_object_keys(&value, &mut keys);
    keys.sort();
    keys.dedup();

    let mut dictionary = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() { dictionary.insert(alias(i), key.clone()); }

    let mut aliased = compact;
    for (a, key) in &dictionary {
        aliased = aliased.replace(&format!("\"{key}\":"), &format!("\"{a}\":"));
    }
    let dict_json = serde_json::to_string(&dictionary).ok()?;
    let wire = format!("SJSON1\n{dict_json}\n{aliased}");
    (wire.len() < raw.len()).then_some(wire)
}

fn restore_json_aliases(compact: &str, dictionary: &BTreeMap<String, String>) -> String {
    let mut out = compact.to_string();
    for (alias, key) in dictionary {
        out = out.replace(&format!("\"{alias}\":"), &format!("\"{key}\":"));
    }
    out
}

fn looks_like_toml(raw: &str) -> bool {
    let Ok(value) = raw.parse::<toml::Value>() else { return false };
    value.as_table().is_some()
}

fn compact_toml(raw: &str) -> Option<String> {
    let mut out = String::new();
    for line in raw.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() { continue; }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    let wire = format!("STOML1\n{out}");
    (wire.len() < raw.len()).then_some(wire)
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
    fn json_nested_aliases_are_semantically_verified() {
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
    fn toml_comments_are_removed_and_semantics_are_verified() {
        let raw = "# header\n[server]\nhost = \"localhost\" # keep inline\nport = 8080\n";
        let codec = FormatCondenser;
        let wire = codec.encode(raw).unwrap();
        assert!(wire.starts_with("STOML1\n"));
        assert!(codec.verify(raw, &wire));
        let decoded = codec.decode(&wire).unwrap();
        assert!(decoded.contains("host = \"localhost\" # keep inline"));
        assert!(!decoded.contains("# header"));
    }
}
