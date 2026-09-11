use super::Diagnosis;

pub fn render_json(d: &Diagnosis) -> String {
    serde_json::json!({
        "samples": d.samples,
        "first_t": d.first_t,
        "last_t": d.last_t,
        "demotes": d.demotes,
        "max_vram_other": d.max_vram_other,
        "max_swap_used": d.max_swap_used,
        "max_page_io_s": d.max_page_io_s,
        "flags": d.flags,
        "timeline": d.timeline,
        "recommendations": d.recommendations,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_json_formats_correctly() {
        let diagnosis = Diagnosis {
            samples: 10,
            first_t: Some(100),
            last_t: Some(200),
            demotes: 5,
            max_vram_other: Some(1024),
            max_swap_used: Some(2048),
            max_page_io_s: Some(500),
            flags: vec!["stuck_slice".to_string()],
            timeline: vec!["event 1".to_string()],
            recommendations: vec!["recommendation 1".to_string()],
        };

        let json_str = render_json(&diagnosis);
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).unwrap_or_else(|e| panic!("failed to parse json: {e}"));

        assert_eq!(parsed["samples"], 10);
        assert_eq!(parsed["first_t"], 100);
        assert_eq!(parsed["last_t"], 200);
        assert_eq!(parsed["demotes"], 5);
        assert_eq!(parsed["max_vram_other"], 1024);
        assert_eq!(parsed["max_swap_used"], 2048);
        assert_eq!(parsed["max_page_io_s"], 500);
        assert_eq!(parsed["flags"][0], "stuck_slice");
        assert_eq!(parsed["timeline"][0], "event 1");
        assert_eq!(parsed["recommendations"][0], "recommendation 1");
    }
}
