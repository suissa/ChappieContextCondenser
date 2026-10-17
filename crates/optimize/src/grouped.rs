use std::collections::HashSet;

const HEADER: &str = "SWGRP";

// Readable inline compression for path listings and grep output. YAML is handled first because it is
// another structured text surface where repeated keys and indentation dominate tool-output tokens.
// The YAML codec proves semantic equality itself before this function returns a candidate.
pub fn encode(raw: &str) -> Option<String> {
    if let Some(wire) = crate::yaml_condenser::YamlCondenser::encode(raw) {
        return Some(wire);
    }

    let (body, trailing) = match raw.strip_suffix('\n') {
        Some(b) => (b, true),
        None => (raw, false),
    };
    if body.is_empty() {
        return None;
    }
    let lines: Vec<&str> = body.split('\n').collect();
    if lines.len() < 3 {
        return None;
    }
    let wire = encode_listing(&lines, trailing).or_else(|| encode_grep_dirs(&lines, trailing))?;
    (decode(&wire).as_deref() == Some(raw)).then_some(wire)
}

fn encode_grep_dirs(lines: &[&str], trailing: bool) -> Option<String> {
    if !lines.iter().all(|l| is_grep_shaped(l)) {
        return None;
    }
    let dirs: Vec<&str> = lines
        .iter()
        .map(|l| dir_of(&l[..l.find(':').unwrap()]))
        .collect();
    if dirs.iter().collect::<HashSet<_>>().len() == lines.len() {
        return None;
    }
    let mut out = format!("{HEADER} {}", trailing as u8);
    let mut cur = "\0";
    for (l, d) in lines.iter().zip(&dirs) {
        if *d != cur {
            out.push('\n');
            out.push_str(d);
            cur = d;
        }
        out.push_str("\n\t");
        out.push_str(&l[d.len()..]);
    }
    Some(out)
}

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "",
    }
}

fn is_grep_shaped(line: &str) -> bool {
    line.match_indices(':').any(|(i, _)| {
        let rest = &line.as_bytes()[i + 1..];
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        digits > 0 && rest.get(digits) == Some(&b':')
    })
}

fn encode_listing(lines: &[&str], trailing: bool) -> Option<String> {
    if lines
        .iter()
        .any(|l| l.contains('\t') || l.is_empty() || is_grep_shaped(l))
        || !lines.iter().any(|l| l.contains('/'))
    {
        return None;
    }
    let dirs: Vec<&str> = lines.iter().map(|l| dir_of(l)).collect();
    if dirs.iter().collect::<HashSet<_>>().len() == lines.len() {
        return None;
    }
    let mut out = format!("{HEADER} {}", trailing as u8);
    let mut cur = "\0";
    for (l, d) in lines.iter().zip(&dirs) {
        if *d != cur {
            out.push('\n');
            out.push_str(d);
            cur = d;
        }
        out.push_str("\n\t");
        out.push_str(&l[d.len()..]);
    }
    Some(out)
}

pub fn decode(wire: &str) -> Option<String> {
    let mut lines = wire.split('\n');
    let rest = lines.next()?.strip_prefix(HEADER)?.strip_prefix(' ')?;
    if rest != "0" && rest != "1" {
        return None;
    }
    let trailing = rest == "1";
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in lines {
        match line.strip_prefix('\t') {
            Some(base) => out.push(format!("{cur}{base}")),
            None => cur = line.to_string(),
        }
    }
    let mut s = out.join("\n");
    if trailing {
        s.push('\n');
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(raw: &str) {
        if let Some(wire) = encode(raw) {
            if wire.starts_with("SWGRP") {
                assert_eq!(decode(&wire).as_deref(), Some(raw), "wire:\n{wire}");
            } else {
                assert!(crate::yaml_condenser::YamlCondenser::verify(raw, &wire));
            }
        }
    }

    #[test]
    fn groups_listing_by_directory() {
        let raw = "a/b/x.rs\na/b/y.rs\na/c/z.rs\nREADME.md\n";
        let wire = encode(raw).unwrap();
        assert!(wire.contains("SWGRP 1") && wire.contains("a/b/"));
        assert_eq!(decode(&wire).as_deref(), Some(raw));
    }

    #[test]
    fn root_files_and_no_trailing_newline_round_trip() {
        round_trip("a/b/x.rs\na/b/y.rs\ntop.txt");
    }

    #[test]
    fn declines_without_repetition() {
        assert!(encode("a/x.rs\nb/y.rs\nc/z.rs\n").is_none());
    }

    #[test]
    fn groups_grep_by_directory_keeping_filename_local() {
        let raw = "src/lib.rs:1:let x = 1\nsrc/lib.rs:9:fn main() {\nsrc/mod.rs:4:use std;\n";
        let wire = encode(raw).unwrap();
        assert!(wire.contains("src/") && wire.contains("lib.rs:1:let x = 1"));
        assert_eq!(decode(&wire).as_deref(), Some(raw));
    }

    #[test]
    fn grep_content_with_slashes_uses_the_path_directory() {
        round_trip("src/a.rs:5:let p = \"a/b/c\";\nsrc/a.rs:6:// http://x\nsrc/z.rs:1:ok\n");
    }

    #[test]
    fn yaml_uses_the_structured_condenser() {
        let raw = "# comment\nservices:\n  app:\n    environment:\n      NODE_ENV: production # inline\n      DATABASE_HOST: db\n      AUTH_SECRET: \"a#b\"\n";
        let wire = encode(raw).unwrap();
        assert!(wire.starts_with("SWY1\n"));
        assert!(!wire.contains("comment"));
        assert!(!wire.contains("inline"));
        assert!(crate::yaml_condenser::YamlCondenser::verify(raw, &wire));
    }
}
