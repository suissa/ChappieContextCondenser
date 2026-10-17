use crate::{
    format_condenser::FormatCondenser,
    lines,
    transform::TextProposer,
    yaml_condenser::YamlCondenser,
};

const COMPOSITE_HEADER: &str = "SFM1";

/// Best-of-N text format matrix. Semantic codecs are attempted for JSON/TOML/YAML; a reversible
/// repeated-line dictionary is available for CSV, XML, HTML, Markdown, source, logs, and unknown
/// text. Semantic wires are also tried through the line dictionary as a second-stage composition.
/// Every candidate is decoded and verified before it can be returned.
pub struct FormatMatrix;

impl TextProposer for FormatMatrix {
    fn id(&self) -> &'static str { "format-matrix" }

    fn encode(&self, raw: &str) -> Option<String> {
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

        // Composition search: apply the lossless line dictionary to semantic wires. This can
        // compress repeated structure introduced or retained by a format-specific codec.
        let semantic: Vec<(String, &'static str)> = candidates.iter()
            .filter(|(_, id)| *id != "lines")
            .cloned()
            .collect();
        for (inner, id) in semantic {
            let Some(outer) = lines::encode(&inner) else { continue };
            let wire = format!("{COMPOSITE_HEADER}\\n{id}\\n{outer}");
            if Self::verify(raw, &wire) {
                candidates.push((wire, "composed"));
            }
        }

        candidates.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.1.cmp(b.1)));
        candidates.into_iter().next().map(|(wire, _)| wire)
    }

    fn decode(&self, wire: &str) -> Option<String> {
        if let Some(rest) = wire.strip_prefix(&format!("{COMPOSITE_HEADER}\\n")) {
            let (id, outer) = rest.split_once('\\n')?;
            let inner_wire = lines::decode(outer)?;
            return match id {
                "format-condenser" => FormatCondenser.decode(&inner_wire),
                "yaml-condenser" => YamlCondenser::decode(&inner_wire),
                _ => None,
            };
        }
        if wire.starts_with("SJSON1\\n") || wire.starts_with("STOML1\\n") {
            return FormatCondenser.decode(wire);
        }
        if wire.starts_with("SWY1\\n") {
            return YamlCondenser::decode(wire);
        }
        if wire.starts_with("[lines]\\n") {
            return lines::decode(wire);
        }
        None
    }

    fn verify(&self, raw: &str, wire: &str) -> bool {
        if let Some(rest) = wire.strip_prefix(&format!("{COMPOSITE_HEADER}\\n")) {
            let Some((id, outer)) = rest.split_once('\\n') else { return false };
            let Some(inner_wire) = lines::decode(outer) else { return false };
            return match id {
                "format-condenser" => FormatCondenser.verify(raw, &inner_wire),
                "yaml-condenser" => YamlCondenser::verify(raw, &inner_wire),
                _ => false,
            };
        }
        if wire.starts_with("SJSON1\\n") || wire.starts_with("STOML1\\n") {
            return FormatCondenser.verify(raw, wire);
        }
        if wire.starts_with("SWY1\\n") {
            return YamlCondenser::verify(raw, wire);
        }
        if wire.starts_with("[lines]\\n") {
            return lines::decode(wire).as_deref() == Some(raw);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::TextProposer;

    #[test]
    fn lossless_line_codec_covers_generic_target_formats() {
        let c = FormatMatrix;
        let samples = [
            ("csv", "name,city,status\\nAna,Sao Paulo,active\\nBia,Santos,active\\nCaio,Itarare,active\\n"),
            ("xml", "<root>\\n  <service name=\\"api\\">\\n    <status>ready</status>\\n  </service>\\n  <service name=\\"web\\">\\n    <status>ready</status>\\n  </service>\\n</root>\\n"),
            ("html", "<main>\\n  <section class=\\"card\\">\\n    <h2>Title</h2>\\n  </section>\\n  <section class=\\"card\\">\\n    <h2>Title</h2>\\n  </section>\\n</main>\\n"),
            ("markdown", "## Service\\n\\n- status: ready\\n- status: ready\\n- status: ready\\n"),
            ("log", "level=INFO service=api status=ready\\nlevel=INFO service=api status=ready\\nlevel=INFO service=api status=ready\\n"),
            ("source", "function ready() {\\n  return true;\\n}\\nfunction ready() {\\n  return true;\\n}\\n"),
        ];
        for (kind, raw) in samples {
            let wire = c.encode(raw).unwrap_or_else(|| panic!("{kind} sample should have a reversible candidate"));
            assert!(c.verify(raw, &wire), "{kind} candidate must verify");
            assert_eq!(c.decode(&wire).as_deref(), Some(raw), "{kind} must restore byte-exactly");
        }
    }

    #[test]
    fn composes_semantic_codec_with_line_dictionary_when_profitable() {
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
        assert_eq!(serde_json::from_str::<serde_json::Value>(&c.decode(&wire).unwrap()).unwrap(),
                   serde_json::from_str::<serde_json::Value>(raw).unwrap());
    }

    #[test]
    fn unknown_or_malformed_wire_is_rejected() {
        let c = FormatMatrix;
        assert!(!c.verify("anything", "SFM1\\nunknown\\nwire"));
        assert!(c.decode("SFM1\\nunknown\\nwire").is_none());
    }
}
