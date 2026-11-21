//! Offline HTML guide generated from the embedded Help topics (text only).

use std::path::Path;

use any2odyssey_core::help::{paragraphs, topics};
use html_escape::{encode_double_quoted_attribute, encode_text};

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
        // Attribute values need the quote escaped as well; `encode_text`
        // only covers what a text node needs.
        html.push_str(&format!(
            "<a href=\"#{}\">{}</a>",
            encode_double_quoted_attribute(&t.id),
            encode_text(&t.title)
        ));
    }
    html.push_str("</nav>\n");
    for t in topics() {
        html.push_str(&format!(
            "<h2 id=\"{}\">{}</h2>\n",
            encode_double_quoted_attribute(&t.id),
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
                encode_double_quoted_attribute(&l.url),
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

/// Write the guide to `path`. An error names the file: the system's own
/// text does not, and "File exists (os error 17)" was all `--export-guide`
/// said of a path that leads through a file.
///
/// The file written is the one the viewer's export would write
/// (`resolve_export`). The guide was renamed into place over whatever had
/// the name: a link was replaced by a file instead of what it leads to
/// being written, and a file the user may not write to was replaced all
/// the same and could be written to afterwards, both with exit code 0.
/// Links are followed now, a file that cannot be written to is left as it
/// is with an error, and one that is replaced keeps its permissions.
pub fn export(path: &Path) -> std::io::Result<()> {
    let failed = |kind, e: &dyn std::fmt::Display| {
        std::io::Error::new(kind, format!("Could not write {}: {e}", path.display()))
    };
    let target =
        crate::ui::resolve_export(path).map_err(|e| failed(std::io::ErrorKind::Other, &e))?;
    any2odyssey_core::fsutil::write_atomic_document_with(&target, |writer| {
        std::io::Write::write_all(writer, render().as_bytes())
    })
    .map_err(|e| failed(e.kind(), &e))
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
    fn attribute_values_escape_the_quote_that_text_nodes_keep() {
        let hostile = r#"x" onmouseover="alert(1)"#;
        assert_eq!(encode_text(hostile), hostile, "text nodes keep quotes");
        let attribute = encode_double_quoted_attribute(hostile);
        assert!(!attribute.contains('"'), "{attribute}");
        // Today's topics need no escaping at all, so the committed guide is
        // unchanged by the stricter encoder.
        for t in topics() {
            assert_eq!(encode_double_quoted_attribute(&t.id), t.id);
            for l in &t.links {
                assert_eq!(encode_double_quoted_attribute(&l.url), l.url);
            }
        }
    }

    #[test]
    fn an_export_that_fails_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        std::fs::write(&file, b"a file, not a folder").unwrap();
        let target = file.join("guide.html");
        let error = export(&target).unwrap_err().to_string();
        assert!(
            error.starts_with(&format!("Could not write {}: ", target.display())),
            "{error}"
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"a file, not a folder");
        let target = dir.path().join("guide.html");
        export(&target).unwrap();
        assert!(std::fs::read_to_string(target).unwrap().contains("</html>"));
    }

    #[cfg(unix)]
    #[test]
    fn an_export_onto_a_link_writes_what_the_link_leads_to() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let served = dir.path().join("served");
        std::fs::create_dir(&served).unwrap();
        let file = served.join("guide.html");
        std::fs::write(&file, b"last version's guide").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("guide.html");
        std::os::unix::fs::symlink("served/guide.html", &link).unwrap();
        export(&link).unwrap();
        assert!(link.is_symlink(), "the link is still a link");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), render());
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640,
            "the file that was replaced keeps its permissions"
        );
        // Nothing is left beside either of them.
        assert_eq!(std::fs::read_dir(&served).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        // A link to a file that is not there yet makes that file.
        let new = dir.path().join("new.html");
        std::os::unix::fs::symlink("served/new.html", &new).unwrap();
        export(&new).unwrap();
        assert!(new.is_symlink());
        assert_eq!(
            std::fs::read_to_string(served.join("new.html")).unwrap(),
            render()
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_export_onto_a_file_that_may_not_be_written_leaves_it_as_it_is() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("guide.html");
        std::fs::write(&file, b"kept").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        // (Whoever may write everything, root, is not kept out.)
        let kept_out = std::fs::OpenOptions::new().write(true).open(&file).is_err();
        let exported = export(&file);
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o444,
            "the file can be written to no more than before"
        );
        if kept_out {
            let error = exported.unwrap_err().to_string();
            assert!(
                error.starts_with(&format!("Could not write {}: ", file.display())),
                "{error}"
            );
            assert!(error.contains("cannot be written to"), "{error}");
            assert_eq!(std::fs::read(&file).unwrap(), b"kept");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
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
