use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::transform::TextProposer;

/// Semantic JSON/TOML condenser. JSON keys are rewritten structurally (never with string replace),
/// and TOML comments/blank lines are omitted only when the parsed TOML value remains equal.
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
            let (dictionary, compact) = body.split_once('\n')?;
            let dictionary: BTreeMap<String, String> = serde_json::from_str(dictionary).ok()?;
            let value: Value = serde_json::from_str(compact).ok()?;
            let reverse: Map<String, Value> = dictionary
                .into_iter()
                .map(|(alias, original)| (alias, Value::String(original)))
                .collect();
            let restored = restore_json_keys(&value, &reverse)?;
            return serde_json::to_string(&restored).ok();
        }
        wire.strip_prefix("STOML1\n").map(str::to_owned)
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        let Some(decoded) = self.decode(wire) else {
            return false;
        };
        if wire.starts_with("SJSON1\n") {
            if crate::atom::has_duplicate_keys(raw) {
                return false;
            }
            let (Ok(original), Ok(restored)) = (
                serde_json::from_str::<Value>(raw),
                serde_json::from_str::<Value>(&decoded),
            ) else {
                return false;
            };
            return original == restored;
        }
        if wire.starts_with("STOML1\n") {
            let (Ok(original), Ok(restored)) =
                (raw.parse::<toml::Value>(), decoded.parse::<toml::Value>())
            else {
                return false;
            };
            return original == restored;
        }
        false
    }
}

fn looks_like_json(raw: &str) -> bool {
    if crate::atom::has_duplicate_keys(raw) {
        return false;
    }
    let trimmed = raw.trim_start();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn collect_object_keys(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key.len() > 3 {
                    keys.insert(key.clone());
                }
                collect_object_keys(child, keys);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_object_keys(child, keys);
            }
        }
        _ => {}
    }
}

fn compact_json(raw: &str) -> Option<String> {
    if crate::atom::has_duplicate_keys(raw) {
        return None;
    }
    let value = serde_json::from_str::<Value>(raw).ok()?;

    let mut long_keys = BTreeSet::new();
    collect_object_keys(&value, &mut long_keys);
    let all_keys = collect_all_keys(&value);
    let mut aliases = BTreeMap::new();
    let mut dictionary = BTreeMap::new();
    let mut index = 0usize;
    for key in long_keys {
        let alias = loop {
            let candidate = alias(index)?;
            index += 1;
            if !all_keys.contains(&candidate) && !dictionary.contains_key(&candidate) {
                break candidate;
            }
        };
        aliases.insert(key.clone(), alias.clone());
        dictionary.insert(alias, key);
    }

    let rewritten = rewrite_json_keys(&value, &aliases)?;
    let body = serde_json::to_string(&rewritten).ok()?;
    let dictionary_json = serde_json::to_string(&dictionary).ok()?;
    let wire = format!("SJSON1\n{dictionary_json}\n{body}");
    let condenser = FormatCondenser;
    (wire.len() < raw.len() && condenser.verify(raw, &wire)).then_some(wire)
}

fn collect_all_keys(value: &Value) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    collect_all_keys_into(value, &mut keys);
    keys
}

fn collect_all_keys_into(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                keys.insert(key.clone());
                collect_all_keys_into(child, keys);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_all_keys_into(child, keys);
            }
        }
        _ => {}
    }
}

fn rewrite_json_keys(value: &Value, aliases: &BTreeMap<String, String>) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut rewritten = Map::new();
            for (key, child) in object {
                let new_key = aliases.get(key).unwrap_or(key);
                if rewritten
                    .insert(new_key.clone(), rewrite_json_keys(child, aliases)?)
                    .is_some()
                {
                    return None;
                }
            }
            Some(Value::Object(rewritten))
        }
        Value::Array(items) => items
            .iter()
            .map(|child| rewrite_json_keys(child, aliases))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        scalar => Some(scalar.clone()),
    }
}

fn restore_json_keys(value: &Value, dictionary: &Map<String, Value>) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut restored = Map::new();
            for (key, child) in object {
                let new_key = dictionary
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or(key);
                if restored
                    .insert(new_key.to_owned(), restore_json_keys(child, dictionary)?)
                    .is_some()
                {
                    return None;
                }
            }
            Some(Value::Object(restored))
        }
        Value::Array(items) => items
            .iter()
            .map(|child| restore_json_keys(child, dictionary))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        scalar => Some(scalar.clone()),
    }
}

fn looks_like_toml(raw: &str) -> bool {
    raw.parse::<toml::Value>()
        .map(|value| value.is_table())
        .unwrap_or(false)
}

fn compact_toml(raw: &str) -> Option<String> {
    let mut body = String::new();
    for line in raw.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        body.push_str(line.trim_end());
        body.push('\n');
    }
    let wire = format!("STOML1\n{body}");
    let condenser = FormatCondenser;
    (wire.len() < raw.len() && condenser.verify(raw, &wire)).then_some(wire)
}

fn alias(mut index: usize) -> Option<String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut output = String::new();
    loop {
        output.push(*ALPHABET.get(index % ALPHABET.len())? as char);
        index /= ALPHABET.len();
        if index == 0 {
            break;
        }
    }
    Some(output.chars().rev().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_nested_key_aliases_round_trip_without_rewriting_values() {
        let raw = r#"[
  {"customerIdentifier":"C123","customerDisplayName":"Ana","customerStatus":"active","accountRegion":"east"},
  {"customerIdentifier":"C456","customerDisplayName":"Bia","customerStatus":"inactive","accountRegion":"west"},
  {"customerIdentifier":"C789","customerDisplayName":"Caio","customerStatus":"active","accountRegion":"east"}
]"#;
        let condenser = FormatCondenser;
        let wire = condenser.encode(raw).expect("repeated long keys should save space");
        assert!(wire.len() < raw.len());
        assert!(condenser.verify(raw, &wire));
        let decoded = condenser.decode(&wire).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&decoded).unwrap(),
            serde_json::from_str::<Value>(raw).unwrap()
        );
    }

    #[test]
    fn duplicate_json_keys_are_declined() {
        let raw = r#"{"status":"first","status":"second","customerStatus":"active"}"#;
        assert!(FormatCondenser.encode(raw).is_none());
    }

    #[test]
    fn toml_comment_lines_round_trip_semantically() {
        let raw = "# header\n[server]\nhost = \"localhost\" # inline\nport = 8080\n";
        let condenser = FormatCondenser;
        let wire = condenser.encode(raw).unwrap();
        assert!(wire.len() < raw.len());
        assert!(condenser.verify(raw, &wire));
    }

    #[test]
    fn quoted_key_names_in_values_are_never_rewritten() {
        let raw = r#"[
  {"customerIdentifier":"the literal customerIdentifier string"},
  {"customerIdentifier":"another customerIdentifier string"},
  {"customerIdentifier":"a third customerIdentifier string"}
]"#;
        let condenser = FormatCondenser;
        let wire = condenser.encode(raw).unwrap();
        assert!(condenser.verify(raw, &wire));
    }
}
