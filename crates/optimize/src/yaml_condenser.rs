use std::collections::{BTreeMap, BTreeSet};

use serde_yaml::Value;

use crate::transform::TextProposer;

const HEADER: &str = "SWY1";
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// YAML condenser for model-facing tool output. Comments are removed because they are not part of
/// the YAML data model; indentation is encoded as `|<next-line-space-count>` and mapping keys longer
/// than three characters get deterministic aliases of at most three characters. YAML block scalars
/// are intentionally declined until their indentation-sensitive bodies have a dedicated encoder.
pub struct YamlCondenser;

impl YamlCondenser {
    pub fn encode(raw: &str) -> Option<String> {
        let _: Value = serde_yaml::from_str(raw).ok()?;
        let mut keys = BTreeSet::new();
        let mut lines = Vec::new();
        let mut block_scalar = false;

        for line in raw.lines() {
            let indent = line.bytes().take_while(|b| *b == b' ').count();
            let stripped = strip_yaml_comment(&line[indent..]);
            if stripped.trim().is_empty() {
                continue;
            }
            let text = stripped.trim_end().to_string();
            if block_scalar {
                return None;
            }
            if has_block_scalar_indicator(&text) {
                block_scalar = true;
            }
            if let Some(key) = mapping_key(&text) {
                if key.len() > 3 {
                    keys.insert(key.to_string());
                }
            }
            lines.push((indent, text));
        }
        if lines.len() < 2 || keys.is_empty() || block_scalar {
            return None;
        }

        let aliases = make_aliases(&keys)?;
        let dictionary = aliases
            .iter()
            .map(|(key, alias)| (alias.clone(), key.clone()))
            .collect::<BTreeMap<_, _>>();

        let mut body = String::new();
        for (i, (_, line)) in lines.iter().enumerate() {
            body.push_str(&rewrite_mapping_key(line, &aliases));
            let next_indent = lines.get(i + 1).map(|(n, _)| *n).unwrap_or(0);
            body.push('|');
            body.push_str(&next_indent.to_string());
            if i + 1 < lines.len() {
                body.push('\n');
            }
        }
        let dict_json = serde_json::to_string(&dictionary).ok()?;
        let wire = format!("{HEADER}\n{dict_json}\n{body}");
        Self::verify(raw, &wire).then_some(wire)
    }

    pub fn decode(wire: &str) -> Option<String> {
        let mut parts = wire.splitn(3, '\n');
        if parts.next()? != HEADER {
            return None;
        }
        let dictionary: BTreeMap<String, String> = serde_json::from_str(parts.next()?).ok()?;
        let body = parts.next()?;
        let mut out = String::new();
        let mut indent = 0usize;
        for (i, encoded) in body.split('\n').enumerate() {
            let (content, marker) = encoded.rsplit_once('|')?;
            let next_indent = marker.parse::<usize>().ok()?;
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&" ".repeat(indent));
            out.push_str(&restore_mapping_key(content, &dictionary));
            indent = next_indent;
        }
        Some(out)
    }

    /// Semantic proof: YAML comments and formatting are deliberately discarded, but parsed values
    /// must remain exactly equal. This is the proof used by the text-proposer admission gate.
    pub fn verify(raw: &str, wire: &str) -> bool {
        let Some(decoded) = Self::decode(wire) else { return false };
        match (serde_yaml::from_str::<Value>(raw), serde_yaml::from_str::<Value>(&decoded)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// Restores known short mapping keys in a YAML response without changing values or unknown keys.
    pub fn restore_response_keys(wire: &str, response: &str) -> Option<String> {
        let mut parts = wire.splitn(3, '\n');
        if parts.next()? != HEADER {
            return None;
        }
        let dictionary: BTreeMap<String, String> = serde_json::from_str(parts.next()?).ok()?;
        let mut out = String::with_capacity(response.len());
        for (i, line) in response.lines().enumerate() {
            if i > 0 { out.push('\n'); }
            let indent = line.bytes().take_while(|b| *b == b' ').count();
            out.push_str(&line[..indent]);
            out.push_str(&restore_mapping_key(&line[indent..], &dictionary));
        }
        Some(out)
    }
}

impl TextProposer for YamlCondenser {
    fn id(&self) -> &'static str { "yaml-condenser" }
    fn encode(&self, raw: &str) -> Option<String> { Self::encode(raw) }
    fn decode(&self, wire: &str) -> Option<String> { Self::decode(wire) }
    fn verify(&self, raw: &str, wire: &str) -> bool { Self::verify(raw, wire) }
}

fn make_aliases(keys: &BTreeSet<String>) -> Option<BTreeMap<String, String>> {
    let mut used = keys.clone();
    let mut result = BTreeMap::new();
    let mut index = 0usize;
    for key in keys {
        loop {
            let alias = alias_for(index)?;
            index += 1;
            if used.insert(alias.clone()) {
                result.insert(key.clone(), alias);
                break;
            }
        }
    }
    Some(result)
}

fn alias_for(mut index: usize) -> Option<String> {
    let mut len = 1usize;
    while index >= ALPHABET.len().pow(len as u32) {
        index -= ALPHABET.len().pow(len as u32);
        len += 1;
        if len > 3 { return None; }
    }
    let mut out = vec![ALPHABET[0]; len];
    for pos in (0..len).rev() {
        out[pos] = ALPHABET[index % ALPHABET.len()];
        index /= ALPHABET.len();
    }
    String::from_utf8(out).ok()
}

fn mapping_key(line: &str) -> Option<&str> {
    let candidate = line.strip_prefix("- ").unwrap_or(line).trim_start();
    let colon = find_unquoted_colon(candidate)?;
    let key = candidate[..colon].trim();
    if key.is_empty() || key.starts_with('&') || key.starts_with('*') || key.starts_with('\'') || key.starts_with('"') {
        return None;
    }
    Some(key)
}

fn rewrite_mapping_key(line: &str, aliases: &BTreeMap<String, String>) -> String {
    let prefix = if line.starts_with("- ") { "- " } else { "" };
    let candidate = &line[prefix.len()..];
    let Some(colon) = find_unquoted_colon(candidate) else { return line.to_string() };
    let key = candidate[..colon].trim();
    let Some(alias) = aliases.get(key) else { return line.to_string() };
    format!("{prefix}{alias}:{}", &candidate[colon + 1..])
}

fn restore_mapping_key(line: &str, dictionary: &BTreeMap<String, String>) -> String {
    let prefix = if line.starts_with("- ") { "- " } else { "" };
    let candidate = &line[prefix.len()..];
    let Some(colon) = find_unquoted_colon(candidate) else { return line.to_string() };
    let key = candidate[..colon].trim();
    let Some(original) = dictionary.get(key) else { return line.to_string() };
    format!("{prefix}{original}:{}", &candidate[colon + 1..])
}

fn find_unquoted_colon(s: &str) -> Option<usize> {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (i, byte) in s.bytes().enumerate() {
        if double && escaped { escaped = false; continue; }
        match byte {
            b'\\' if double => escaped = true,
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b':' if !single && !double => return Some(i),
            _ => {}
        }
    }
    None
}

// A # starts a comment only outside quoted scalars and at the beginning of a token. A # inside
// a quoted string, such as "a#b", remains data.
fn strip_yaml_comment(s: &str) -> String {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (i, byte) in s.bytes().enumerate() {
        if double && escaped { escaped = false; continue; }
        match byte {
            b'\\' if double => escaped = true,
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'#' if !single && !double && (i == 0 || s.as_bytes()[i - 1].is_ascii_whitespace()) => return s[..i].trim_end().to_string(),
            _ => {}
        }
    }
    s.trim_end().to_string()
}

fn has_block_scalar_indicator(line: &str) -> bool {
    let Some(colon) = find_unquoted_colon(line) else { return false };
    let value = line[colon + 1..].trim();
    matches!(value, "|" | ">" | "|-" | "|+" | ">-" | ">+")
}

#[cfg(test)]
mod tests {
    use super::YamlCondenser;

    const SAMPLE: &str = r#"# top comment
services:
  app:
    environment:
      NODE_ENV: production # inline comment
      DATABASE_HOST: db
      DATABASE_PORT: 5432
      AUTH_SECRET: "a#b"
  db:
    image: postgres:18
    restart: unless-stopped
"#;

    #[test]
    fn removes_comments_and_compacts_indentation() {
        let wire = YamlCondenser::encode(SAMPLE).unwrap();
        assert!(wire.starts_with("SWY1\n"));
        assert!(!wire.contains("top comment"));
        assert!(!wire.contains("inline comment"));
        assert!(wire.contains("|2"));
        assert!(wire.contains("a#b"));
        assert!(YamlCondenser::verify(SAMPLE, &wire));
    }

    #[test]
    fn aliases_mapping_keys_and_round_trip_semantically() {
        let wire = YamlCondenser::encode(SAMPLE).unwrap();
        let decoded = YamlCondenser::decode(&wire).unwrap();
        assert!(decoded.contains("services:"));
        assert!(decoded.contains("DATABASE_HOST:"));
        assert!(!decoded.contains("top comment"));
        assert!(YamlCondenser::verify(SAMPLE, &wire));
    }

    #[test]
    fn restores_known_keys_in_response() {
        let wire = YamlCondenser::encode(SAMPLE).unwrap();
        let response = YamlCondenser::decode(&wire).unwrap();
        let restored = YamlCondenser::restore_response_keys(&wire, &response).unwrap();
        assert!(restored.contains("services:"));
        assert!(restored.contains("DATABASE_HOST:"));
    }

    #[test]
    fn does_not_treat_hash_inside_quotes_as_comment() {
        let wire = YamlCondenser::encode("a: \"x#y\"\nb: 1\n").unwrap();
        assert!(wire.contains("x#y"));
    }

    #[test]
    fn declines_block_scalars_until_their_indentation_is_encoded_safely() {
        assert!(YamlCondenser::encode("a: |\n  hello\nb: 1\n").is_none());
    }
}
