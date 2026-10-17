use serde_json::{Map, Value};

use crate::transform::{Encoded, Transform};

const HEADER: &str = "SWVAR1";
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Replaces repeated JSON object keys with short deterministic aliases while keeping a dictionary
/// in the wire. Values and types are untouched; the optimizer's normal admission gate proves that
/// decoding reconstructs the exact original JSON value and only ships the wire when it saves cost.
#[derive(Default)]
pub struct VariablesSimplifier;

impl Transform for VariablesSimplifier {
    fn id(&self) -> &'static str {
        "variables-simplifier"
    }

    fn try_encode(&self, value: &Value) -> Option<Encoded> {
        let mut keys = Vec::new();
        collect_keys(value, &mut keys);
        keys.sort();
        keys.dedup();

        // The dictionary costs more than it saves when there are few keys. Let the shared net-cost
        // gate make the final decision, but avoid building a wire for keyless or tiny structures.
        if keys.len() < 2 {
            return None;
        }

        let mut aliases = Map::new();
        let mut dictionary = Map::new();
        for (index, key) in keys.iter().enumerate() {
            let alias = alias_for(index)?;
            aliases.insert(key.clone(), Value::String(alias.clone()));
            dictionary.insert(alias, Value::String(key.clone()));
        }

        let compact = rewrite_keys(value, &aliases)?;
        let wire = format!(
            "{HEADER}\n{}\n{}",
            Value::Object(dictionary),
            compact
        );
        let decoded = decode(&wire)?;
        Some(Encoded { wire, decoded })
    }
}

/// Restores aliased keys in a JSON model response using the dictionary from a VariablesSimplifier wire.
/// This is an explicit host-side helper; the proxy response path does not call it automatically.
pub fn restore_response_keys(wire: &str, response_json: &str) -> Option<String> {
    let mut parts = wire.splitn(3, '\n');
    if parts.next()? != HEADER {
        return None;
    }
    let dictionary: Value = serde_json::from_str(parts.next()?).ok()?;
    let dictionary = dictionary.as_object()?;
    let response: Value = serde_json::from_str(response_json).ok()?;
    let mut restored = Map::new();
    for (alias, original) in dictionary {
        let original = original.as_str()?;
        if restored.insert(alias.clone(), Value::String(original.to_owned())).is_some() {
            return None;
        }
    }
    let restored = restore_response_value(&response, &restored)?;
    serde_json::to_string(&restored).ok()
}

fn restore_response_value(value: &Value, dictionary: &Map<String, Value>) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut result = Map::new();
            for (key, child) in object {
                // Only aliases known to this block are restored. Other response keys are preserved.
                let restored_key = dictionary.get(key).and_then(Value::as_str).unwrap_or(key);
                if result
                    .insert(restored_key.to_owned(), restore_response_value(child, dictionary)?)
                    .is_some()
                {
                    return None;
                }
            }
            Some(Value::Object(result))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| restore_response_value(item, dictionary))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        scalar => Some(scalar.clone()),
    }
}

/// Restores all aliased keys from a VariablesSimplifier wire representation.
pub fn decode(wire: &str) -> Option<Value> {
    let mut parts = wire.splitn(3, '\n');
    if parts.next()? != HEADER {
        return None;
    }
    let dictionary: Value = serde_json::from_str(parts.next()?).ok()?;
    let compact: Value = serde_json::from_str(parts.next()?).ok()?;
    let dictionary = dictionary.as_object()?;
    if dictionary.is_empty() {
        return None;
    }
    let mut reverse = Map::new();
    for (alias, original) in dictionary {
        let original = original.as_str()?;
        if reverse.insert(alias.clone(), Value::String(original.to_owned())).is_some() {
            return None;
        }
    }
    restore_keys(&compact, &reverse)
}

fn collect_keys(value: &Value, keys: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                keys.push(key.clone());
                collect_keys(child, keys);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_keys(item, keys);
            }
        }
        _ => {}
    }
}

fn alias_for(mut index: usize) -> Option<String> {
    // Base-62 aliases, shortest first, deterministic across processes and input order.
    let mut length = 1usize;
    let mut capacity = ALPHABET.len();
    while index >= capacity {
        index -= capacity;
        length += 1;
        if length > 3 {
            return None;
        }
        capacity = capacity.checked_mul(ALPHABET.len())?;
    }
    let mut bytes = vec![ALPHABET[0]; length];
    for position in (0..length).rev() {
        bytes[position] = ALPHABET[index % ALPHABET.len()];
        index /= ALPHABET.len();
    }
    String::from_utf8(bytes).ok()
}

fn rewrite_keys(value: &Value, aliases: &Map<String, Value>) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut result = Map::new();
            for (key, child) in object {
                let alias = aliases.get(key)?.as_str()?;
                if result.insert(alias.to_owned(), rewrite_keys(child, aliases)?).is_some() {
                    return None;
                }
            }
            Some(Value::Object(result))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| rewrite_keys(item, aliases))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        scalar => Some(scalar.clone()),
    }
}

fn restore_keys(value: &Value, dictionary: &Map<String, Value>) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let mut result = Map::new();
            for (alias, child) in object {
                let original = dictionary.get(alias)?.as_str()?;
                if result
                    .insert(original.to_owned(), restore_keys(child, dictionary)?)
                    .is_some()
                {
                    return None;
                }
            }
            Some(Value::Object(result))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| restore_keys(item, dictionary))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        scalar => Some(scalar.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_repeated_nested_keys_and_preserves_values() {
        let value = json!([
            {"customerIdentifier": "C123", "customerDisplayName": "Ana", "account": {"status": "active"}},
            {"customerIdentifier": "C456", "customerDisplayName": "Bia", "account": {"status": "inactive"}}
        ]);
        let encoded = VariablesSimplifier.try_encode(&value).unwrap();
        assert_eq!(decode(&encoded.wire).unwrap(), value);
        assert_eq!(encoded.decoded, value);
        assert!(encoded.wire.contains("customerIdentifier"));
        assert!(encoded.wire.contains("customerDisplayName"));
        assert!(!encoded.wire.contains("\"customerIdentifier\":"));
    }

    #[test]
    fn leaves_strings_that_look_like_keys_unchanged() {
        let value = json!({"longPropertyName": "longPropertyName", "nested": [{"longPropertyName": 42}]});
        let encoded = VariablesSimplifier.try_encode(&value).unwrap();
        assert_eq!(decode(&encoded.wire).unwrap(), value);
    }

    #[test]
    fn aliases_are_deterministic_and_short() {
        let value = json!({"zebraProperty": 1, "alphaProperty": 2, "otherProperty": 3});
        let first = VariablesSimplifier.try_encode(&value).unwrap();
        let second = VariablesSimplifier.try_encode(&value).unwrap();
        assert_eq!(first.wire, second.wire);
        let dictionary: Value = serde_json::from_str(first.wire.splitn(3, '\n').nth(1).unwrap()).unwrap();
        assert!(dictionary.as_object().unwrap().keys().all(|key| key.len() <= 3));
    }

    #[test]
    fn restores_known_aliases_in_model_response_and_preserves_other_keys() {
        let source = json!([{"customerIdentifier": "C123", "status": "active"}]);
        let encoded = VariablesSimplifier.try_encode(&source).unwrap();
        let response = r#"{"a":"C999","unrelated":"kept"}"#;
        let restored = restore_response_keys(&encoded.wire, response).unwrap();
        let value: Value = serde_json::from_str(&restored).unwrap();
        assert_eq!(value, json!({"customerIdentifier": "C999", "unrelated": "kept"}));
    }

    #[test]
    fn refuses_unknown_aliases_and_malformed_wires() {
        assert!(decode("SWVAR1\n{}\n{}").is_none());
        assert!(decode("SWVAR1\n{\"a\":\"known\"}\n{\"b\":1}").is_none());
        assert!(decode("not-a-wire").is_none());
    }

    #[test]
    fn declines_keyless_and_single_key_values() {
        assert!(VariablesSimplifier.try_encode(&json!({"onlyOneKey": {"onlyOneKey": 1}})).is_none());
        assert!(VariablesSimplifier.try_encode(&json!([1, 2, 3])).is_none());
        assert!(VariablesSimplifier.try_encode(&json!({"sameKey": 1})).is_none());
    }
}
