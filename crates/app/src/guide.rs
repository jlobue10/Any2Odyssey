//! Offline HTML guide generated from the embedded Help topics (text only).

use std::path::Path;

use any2odyssey_core::help::{paragraphs, topics};
use html_escape::encode_text;

pub fn render() -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str(&format!(
        "<title>Any2Odyssey {} user guide</title>\n",
        any2odyssey_core::VERSION
    ));
    html.push_str("<style>body{font-family:Georgia,'Palatino Linotype',serif;background:#EEE8DC;color:#232727;max-width:60em;margin:2em auto;padding:0 1em;line-height:1.5}h1,h2{color:#236F7C}h2{border-bottom:1px solid #967028;padding-bottom:.2em;margin-top:2em}.caps{font-weight:bold;color:#654A19}nav a{margin-right:1em}a{color:#236F7C}@media(prefers-color-scheme:dark){body{background:#111315;color:#F2ECE2}h1,h2{color:#7CC4D3}.caps{color:#D6BA70}a{color:#7CC4D3}}</style></head><body>\n");
    html.push_str(&format!("<h1>Any2Odyssey {} — user guide</h1>\n<p>Rust reimplementation of Any2Odd by Zhandragon (GPL-3.0-only). The same topics are available inside the app with F1.</p>\n<nav>", any2odyssey_core::VERSION));
    for t in topics() {
        html.push_str(&format!(
            "<a href=\"#{}\">{}</a>",
            encode_text(&t.id),
            encode_text(&t.title)
        ));
    }
    html.push_str("</nav>\n");
    for t in topics() {
        html.push_str(&format!(
            "<h2 id=\"{}\">{}</h2>\n",
            encode_text(&t.id),
            encode_text(&t.title)
        ));
        for p in paragraphs(&t.body) {
            let mut lines = p.lines();
            let first = lines.next().unwrap_or("");
            if is_caps_heading(first) {
                let rest: Vec<&str> = lines.collect();
                html.push_str(&format!(
                    "<p><span class=\"caps\">{}</span><br>{}</p>\n",
                    encode_text(first),
                    rest.iter()
                        .map(|l| encode_text(l).into_owned())
                        .collect::<Vec<_>>()
                        .join("<br>")
                ));
            } else {
                html.push_str(&format!(
                    "<p>{}</p>\n",
                    encode_text(p).replace('\n', "<br>")
                ));
            }
        }
        for l in &t.links {
            html.push_str(&format!(
                "<p><a href=\"{}\">{}</a></p>\n",
                encode_text(&l.url),
                encode_text(&l.label)
            ));
        }
    }
    html.push_str("</body></html>\n");
    html
}

/// A line of at least four letters with no lowercase letters is a heading.
pub fn is_caps_heading(line: &str) -> bool {
    let letters: Vec<char> = line.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 4 && letters.iter().all(|c| c.is_uppercase()) && line.len() < 90
}

pub fn export(path: &Path) -> std::io::Result<()> {
    any2odyssey_core::fsutil::write_atomic_document(path, render().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_detection() {
        assert!(is_caps_heading("WHOLE DESKTOP"));
        assert!(is_caps_heading("2D → 3D AUTOMATICALLY (AI)"));
        assert!(!is_caps_heading("The recommended mode for games."));
        assert!(!is_caps_heading("1."));
    }

    #[test]
    fn guide_contains_every_topic_and_no_donations() {
        let html = render();
        for t in topics() {
            assert!(html.contains(&format!("id=\"{}\"", t.id)));
        }
        assert!(!html.to_lowercase().contains("donate"));
        assert!(html.contains("Zhandragon"));
    }
}
