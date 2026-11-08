//! Written Help topics, embedded from `help.json` and shown inside the
//! controls window (F1) and exported to the offline guide.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct HelpLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct HelpTopic {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub links: Vec<HelpLink>,
}

pub const HELP_JSON: &str = include_str!("help.json");

/// All topics, parsed once. The JSON is validated by tests, so a parse
/// failure here is a build defect and panics on first use.
pub fn topics() -> &'static [HelpTopic] {
    static TOPICS: OnceLock<Vec<HelpTopic>> = OnceLock::new();
    TOPICS.get_or_init(|| serde_json::from_str(HELP_JSON).expect("help.json is valid"))
}

pub fn topic(id: &str) -> Option<&'static HelpTopic> {
    topics().iter().find(|t| t.id == id)
}

/// Paragraphs of a body (blank-line separated), for rendering.
pub fn paragraphs(body: &str) -> impl Iterator<Item = &str> {
    body.split("\n\n").map(str::trim).filter(|p| !p.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_parse_with_unique_ids_and_a_start_topic() {
        let all = topics();
        assert!(all.len() >= 10);
        let mut ids: Vec<_> = all.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "duplicate topic ids");
        assert!(topic("start").is_some());
        assert!(topic("origins").is_some());
        for t in all {
            assert!(!t.title.is_empty() && !t.body.is_empty(), "{}", t.id);
            for l in &t.links {
                assert!(l.url.starts_with("https://"), "{}: {}", t.id, l.url);
            }
        }
    }

    #[test]
    fn help_is_rebranded_and_carries_no_donation_content() {
        for t in topics() {
            let text = format!("{} {}", t.title, t.body).to_lowercase();
            // Upstream may be named only where migration or credit is explained.
            if !["origins", "automation", "updates", "dependencies", "platforms"].contains(&t.id.as_str()) {
                assert!(
                    !text.contains("any2odd"),
                    "topic {} still mentions Any2Odd",
                    t.id
                );
            }
            assert!(!t.title.to_lowercase().contains("any2odd"));
            for banned in [
                "donate",
                "sponsor",
                "paypal",
                "bitcoin",
                "ethereum",
                "qr code",
                "sharpdx",
                ".net framework",
                "odyssey3dcompanion",
            ] {
                assert!(!text.contains(banned), "topic {} mentions {banned}", t.id);
            }
            for l in &t.links {
                assert!(
                    !l.url.contains("sponsors") && !l.url.contains("paypal"),
                    "{}",
                    l.url
                );
            }
        }
        assert!(topic("origins").unwrap().body.contains("Zhandragon"));
    }

    #[test]
    fn paragraph_splitting() {
        let v: Vec<_> = paragraphs("a\n\n\nb\n\n").collect();
        assert_eq!(v, vec!["a", "b"]);
    }
}
