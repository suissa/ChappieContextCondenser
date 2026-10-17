use std::collections::BTreeMap;
use serde_json::Value;
use crate::transform::TextProposer;

/// Format-aware model-facing condenser. It emits a wire representation only when the
/// corresponding parser can prove that all semantic values survive reconstruction.
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
            let dict: BTreeMap<String, String> = serde_json::from_str(parts.next()?).ok()?;
            return Some(restore_json_aliases(parts.next()?, &dict));
        }
        wire.strip_prefix("STOML1\n").map(str::to_owned)
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        let Some(decoded) = self.decode(wire) else { return false };
        if wire.starts_with("SJSON1\n") {
            let (Ok(a), Ok(b)) = (serde_json::from_str::<Value>(raw), serde_json::from_str::<Value>(&decoded)) else { return false };
            return a == b;
        }
        if wire.starts_with("STOML1\n") {
            let (Ok(a), Ok(b)) = (raw.parse::<toml::Value>(), decoded.parse::<toml::Value>()) else { return false };
            return a == b;
        }
        false
    }
}

fn looks_like_json(raw: &str) -> bool {
    let s = raw.trim_start();
    (s.starts_with('{') && s.ends_with('}')) || (s.starts_with('[') && s.ends_with(']'))
}

fn collect_object_keys(v: &Value, keys: &mut Vec<String>) {
    match v {
        Value::Object(m) => { for (k, child) in m { if k.len() > 3 { keys.push(k.clone()); } collect_object_keys(child, keys); } }
        Value::Array(a) => for child in a { collect_object_keys(child, keys); },
        _ => {}
    }
}

fn compact_json(raw: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let compact = serde_json::to_string(&value).ok()?;
    let mut keys = Vec::new();
    collect_object_keys(&value, &mut keys);
    keys.sort(); keys.dedup();

    let mut dict = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() { dict.insert(alias(i), key.clone()); }
    let mut body = compact;
    for (a, key) in &dict { body = body.replace(&format!("\"{key}\":"), &format!("\"{a}\":")); }
    let wire = format!("SJSON1\n{}\n{}", serde_json::to_string(&dict).ok()?, body);
    (wire.len() < raw.len()).then_some(wire)
}

fn restore_json_aliases(body: &str, dict: &BTreeMap<String, String>) -> String {
    let mut out = body.to_owned();
    for (a, key) in dict { out = out.replace(&format!("\"{a}\":"), &format!("\"{key}\":")); }
    out
}

fn looks_like_toml(raw: &str) -> bool { raw.parse::<toml::Value>().map(|v| v.is_table()).unwrap_or(false) }

fn compact_toml(raw: &str) -> Option<String> {
    let mut body = String::new();
    for line in raw.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') { continue; }
        body.push_str(line.trim_end()); body.push('\n');
    }
    let wire = format!("STOML1\n{body}");
    (wire.len() < raw.len()).then_some(wire)
}

fn alias(mut n: usize) -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::new();
    loop { out.push(A[n % A.len()] as char); n /= A.len(); if n == 0 { break; } }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_nested_keys_round_trip_semantically() {
        let raw = r#"{
  "customer_name": "Ana",
  "customer_address": "Rua A",
  "customer_status": "active",
  "items": [{"customer_name": "Bia", "customer_status": "pending"}]
}"#;
        let c = FormatCondenser;
        let wire = c.encode(raw).unwrap();
        assert!(wire.len() < raw.len());
        assert!(c.verify(raw, &wire));
    }

    #[test]
    fn json_values_are_never_alias_rewritten() {
        let raw = r#"{"message":"customer_name: customer_name", "customer_name":"x"}"#;
        let c = FormatCondenser;
        let wire = c.encode(raw).unwrap();
        assert!(c.verify(raw, &wire));
    }

    #[test]
    fn toml_comment_lines_round_trip_semantically() {
        let raw = "# header\n[server]\nhost = \"localhost\" # inline\nport = 8080\n";
        let c = FormatCondenser;
        let wire = c.encode(raw).unwrap();
        assert!(wire.len() < raw.len());
        assert!(c.verify(raw, &wire));
    }
}
