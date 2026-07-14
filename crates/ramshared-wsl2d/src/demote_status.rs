//! Persist demote counters for CLI `status --json` (cascade-lifecycle-observability ITEM-3).
//!
//! Path: `/run/ramshared/demote-status.json` (same run dir as pid/sock).
//! Pure parse/render unit-tested; write is best-effort.

use std::fs;
use std::io;
use std::path::Path;

/// Default path next to `ramsharedd.pid` / `wsl2d.sock`.
pub const DEMOTE_STATUS_PATH: &str = "/run/ramshared/demote-status.json";

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DemoteStatusFile {
    pub total: u64,
    pub last_reason: Option<String>,
    pub in_progress: bool,
}

/// Render one JSON object (no serde).
pub fn render_demote_status_json(s: &DemoteStatusFile) -> String {
    let reason = match &s.last_reason {
        Some(r) => format!("\"{}\"", json_escape_inner(r)),
        None => "null".into(),
    };
    format!(
        "{{\"total\":{},\"last_reason\":{},\"in_progress\":{}}}",
        s.total,
        reason,
        if s.in_progress { "true" } else { "false" }
    )
}

fn json_escape_inner(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Best-effort write; creates parent dir if needed.
pub fn write_demote_status(path: &Path, s: &DemoteStatusFile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, render_demote_status_json(s))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Parse status file body. Tolerant of whitespace.
pub fn parse_demote_status(text: &str) -> Option<DemoteStatusFile> {
    let t = text.trim();
    if !t.starts_with('{') {
        return None;
    }
    let total = extract_u64(t, "total")?;
    let in_progress = extract_bool(t, "in_progress").unwrap_or(false);
    let last_reason = extract_string_or_null(t, "last_reason");
    Some(DemoteStatusFile {
        total,
        last_reason,
        in_progress,
    })
}

fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\":");
    let i = json.find(&pat)?;
    let rest = json[i + pat.len()..].trim_start();
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse().ok()
}

fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{key}\":");
    let i = json.find(&pat)?;
    let rest = json[i + pat.len()..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn extract_string_or_null(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":");
    let i = json.find(&pat)?;
    let rest = json[i + pat.len()..].trim_start();
    if rest.starts_with("null") {
        return None;
    }
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '"' => break,
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn roundtrip_render_parse() {
        let s = DemoteStatusFile {
            total: 3,
            last_reason: Some("Latency".into()),
            in_progress: true,
        };
        let j = render_demote_status_json(&s);
        let p = parse_demote_status(&j).expect("parse");
        assert_eq!(p, s);
    }

    #[test]
    fn parse_null_reason() {
        let j = r#"{"total":0,"last_reason":null,"in_progress":false}"#;
        let p = parse_demote_status(j).unwrap();
        assert_eq!(p.total, 0);
        assert!(p.last_reason.is_none());
        assert!(!p.in_progress);
    }
}
