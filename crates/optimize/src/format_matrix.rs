use crate::{
    dict,
    format_condenser::FormatCondenser,
    lines,
    transform::TextProposer,
    yaml_condenser::YamlCondenser,
};

const COMPOSITE_HEADER: &str = "SFM1";
const DICT_COMPOSITE_HEADER: &str = "SFM1D";

/// Best-of-N text format matrix. Semantic codecs are attempted for JSON/TOML/YAML; reversible
/// repeated-line and repeated-token dictionaries cover CSV, XML, HTML, Markdown, source, logs,
/// and unknown text. Semantic wires are also tried through both dictionaries as second-stage codecs.
pub struct FormatMatrix;

impl FormatMatrix {
    fn candidates(raw: &str) -> Vec<String> {
        let mut candidates: Vec<(String, &'static str)> = Vec::new();

        if let Some(wire) = FormatCondenser.encode(raw) {
            if FormatCondenser.verify(raw, &wire) {
                candidates.push((wire, "format-condenser"));
            }
        }
        if let Some(wire) = YamlCondenser::encode(raw) {
            if YamlCondenser::verify(raw, &wire) {
                candidates.push((wire, "yaml-condenser"));
            }
        }
        if let Some(wire) = lines::encode(raw) {
            if lines::decode(&wire).as_deref() == Some(raw) {
                candidates.push((wire, "lines"));
            }
        }
        if let Some(wire) = dict::encode(raw) {
            if dict::decode(&wire).as_deref() == Some(raw) {
                candidates.push((wire, "dict"));
            }
        }

        // Composition candidates are independently verified; the optimizer prices all candidates
        // using its configured tokenizer rather than trusting this byte-length ordering.
        let semantic: Vec<(String, &'static str)> = candidates
            .iter()
            .filter(|(_, id)| *id == "format-condenser" || *id == "yaml-condenser")
            .cloned()
            .collect();

        for (inner, id) in semantic {
            if let Some(outer) = lines::encode(&inner) {
                let wire = format!("{COMPOSITE_HEADER}\n{id}\n{outer}");
                if FormatMatrix.verify(raw, &wire) {
                    candidates.push((wire, "composed-lines"));
                }
            }
            if let Some(outer) = dict::encode(&inner) {
                let wire = format!("{DICT_COMPOSITE_HEADER}\n{id}\n{outer}");
                if FormatMatrix.verify(raw, &wire) {
                    candidates.push((wire, "composed-dict"));
                }
            }
        }

        candidates.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.1.cmp(b.1)));
        let mut seen = std::collections::BTreeSet::new();
        candidates
            .into_iter()
            .filter_map(|(wire, _)| seen.insert(wire.clone()).then_some(wire))
            .collect()
    }
}

impl TextProposer for FormatMatrix {
    fn id(&self) -> &'static str {
        "format-matrix"
    }

    fn encode(&self, raw: &str) -> Option<String> {
        Self::candidates(raw).into_iter().next()
    }

    fn encode_candidates(&self, raw: &str) -> Vec<String> {
        Self::candidates(raw)
    }

    fn decode(&self, wire: &str) -> Option<String> {
        if let Some(rest) = wire.strip_prefix(&format!("{DICT_COMPOSITE_HEADER}\n")) {
            let (id, outer) = rest.split_once('\n')?;
            let inner_wire = dict::decode(outer)?;
            return match id {
                "format-condenser" => FormatCondenser.decode(&inner_wire),
                "yaml-condenser" => YamlCondenser::decode(&inner_wire),
                _ => None,
            };
        }
        if let Some(rest) = wire.strip_prefix(&format!("{COMPOSITE_HEADER}\n")) {
            let (id, outer) = rest.split_once('\n')?;
            let inner_wire = lines::decode(outer)?;
            return match id {
                "format-condenser" => FormatCondenser.decode(&inner_wire),
                "yaml-condenser" => YamlCondenser::decode(&inner_wire),
                _ => None,
            };
        }
        if wire.starts_with("SJSON1\n") || wire.starts_with("STOML1\n") {
            return FormatCondenser.decode(wire);
        }
        if wire.starts_with("SWY1\n") {
            return YamlCondenser::decode(wire);
        }
        if wire.starts_with("[lines]\n") {
            return lines::decode(wire);
        }
        if wire.starts_with("[dict]\n") {
            return dict::decode(wire);
        }
        None
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        if let Some(rest) = wire.strip_prefix(&format!("{DICT_COMPOSITE_HEADER}\n")) {
            let Some((id, outer)) = rest.split_once('\n') else {
                return false;
            };
            let Some(inner_wire) = dict::decode(outer) else {
                return false;
            };
            return match id {
                "format-condenser" => FormatCondenser.verify(raw, &inner_wire),
                "yaml-condenser" => YamlCondenser::verify(raw, &inner_wire),
                _ => false,
            };
        }
        if let Some(rest) = wire.strip_prefix(&format!("{COMPOSITE_HEADER}\n")) {
            let Some((id, outer)) = rest.split_once('\n') else {
                return false;
            };
            let Some(inner_wire) = lines::decode(outer) else {
                return false;
            };
            return match id {
                "format-condenser" => FormatCondenser.verify(raw, &inner_wire),
                "yaml-condenser" => YamlCondenser::verify(raw, &inner_wire),
                _ => false,
            };
        }
        if wire.starts_with("SJSON1\n") || wire.starts_with("STOML1\n") {
            return FormatCondenser.verify(raw, wire);
        }
        if wire.starts_with("SWY1\n") {
            return YamlCondenser::verify(raw, wire);
        }
        if wire.starts_with("[lines]\n") {
            return lines::decode(wire).as_deref() == Some(raw);
        }
        if wire.starts_with("[dict]\n") {
            return dict::decode(wire).as_deref() == Some(raw);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::TextProposer;

    #[test]
    fn reversible_codecs_cover_generic_target_formats() {
        let c = FormatMatrix;
        let samples = [
            (
                "csv",
                r#"state,state,state
active,active,active
active,active,active
active,active,active
"#,
            ),
            (
                "xml",
                r###"<root>
  <status>ready</status>
  <status>ready</status>
  <status>ready</status>
</root>
"###,
            ),
            (
                "html",
                r###"<main>
  <h2>Title</h2>
  <h2>Title</h2>
  <h2>Title</h2>
</main>
"###,
            ),
            (
                "markdown",
                "## Service\n\n- status: ready\n- status: ready\n- status: ready\n",
            ),
            (
                "log",
                "level=INFO service=api status=ready\nlevel=INFO service=api status=ready\nlevel=INFO service=api status=ready\n",
            ),
            (
                "source",
                "function ready() {\n  return true;\n}\nfunction ready() {\n  return true;\n}\nfunction ready() {\n  return true;\n}\n",
            ),
        ];
        for (kind, raw) in samples {
            let wire = c
                .encode(raw)
                .unwrap_or_else(|| panic!("{kind} sample should have a reversible candidate"));
            assert!(c.verify(raw, &wire), "{kind} candidate must verify");
            assert_eq!(
                c.decode(&wire).as_deref(),
                Some(raw),
                "{kind} must restore byte-exactly"
            );
        }
    }

    #[test]
    fn json_candidate_round_trips_semantically() {
        let raw = r#"{
  "serviceIdentifier": "api",
  "serviceStatus": "ready",
  "serviceRegion": "east",
  "serviceOwner": "platform",
  "items": [
    {"serviceIdentifier": "web", "serviceStatus": "ready", "serviceRegion": "east", "serviceOwner": "platform"},
    {"serviceIdentifier": "worker", "serviceStatus": "ready", "serviceRegion": "east", "serviceOwner": "platform"}
  ]
}"#;
        let c = FormatMatrix;
        let wire = c.encode(raw).expect("JSON should have a candidate");
        assert!(c.verify(raw, &wire));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&c.decode(&wire).unwrap()).unwrap(),
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
    }

    #[test]
    fn unknown_or_malformed_wire_is_rejected() {
        let c = FormatMatrix;
        assert!(!c.verify("anything", "SFM1\nunknown\nwire"));
        assert!(c.decode("SFM1\nunknown\nwire").is_none());
    }
}
