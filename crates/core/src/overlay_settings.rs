//! Samsung's `OverlaySettings.json` carries the depth/pop-out the engine uses.
//! The worker writes the chosen values into the section matching the input
//! format right before starting the engine, after backing up the original once.

use serde_json::{Map, Value};
use std::path::Path;

pub const SECTION_SBS: &str = "SideBySideVideo";
pub const SECTION_STANDARD: &str = "StandardVideo";

/// Apply depth/pop-out to the in-memory document. Unknown content is preserved.
pub fn apply(doc: &mut Value, side_by_side: bool, depth: f64, popout: f64) {
    if !doc.is_object() {
        *doc = Value::Object(Map::new());
    }
    let section = if side_by_side {
        SECTION_SBS
    } else {
        SECTION_STANDARD
    };
    let root = doc.as_object_mut().expect("object ensured above");
    let values = match root.get_mut(section) {
        Some(Value::Object(_)) => root.get_mut(section).unwrap(),
        _ => {
            root.insert(section.to_owned(), Value::Object(Map::new()));
            root.get_mut(section).unwrap()
        }
    };
    let values = values.as_object_mut().expect("section is an object");
    values.insert("3D Depth".into(), Value::String(format!("{depth:.6}")));
    values.insert("3D Popout".into(), Value::String(format!("{popout:.6}")));
    values.insert("3D Off".into(), Value::Bool(false));
    values.insert("Overlay Off".into(), Value::Bool(true));
}

/// Read Samsung's file, back up a valid original once, and update our section.
/// Only a missing file is initialized. Unreadable, malformed, or unexpected
/// data is left intact so a transient Samsung write cannot erase its settings.
pub fn update_file(
    path: &Path,
    backup: &Path,
    side_by_side: bool,
    depth: f64,
    popout: f64,
) -> std::io::Result<()> {
    use std::io::{self, Write};
    let original = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let mut doc = match original.as_deref() {
        Some(bytes) => serde_json::from_slice::<Value>(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        None => Value::Object(Map::new()),
    };
    let section = if side_by_side {
        SECTION_SBS
    } else {
        SECTION_STANDARD
    };
    if !doc.is_object() || doc.get(section).is_some_and(|value| !value.is_object()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Samsung overlay settings must contain JSON objects; original left unchanged",
        ));
    }
    if let Some(bytes) = original {
        if let Some(parent) = backup.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        // Back up exactly the snapshot we parsed, without replacing a prior backup.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(backup)
        {
            Ok(mut file) => file.write_all(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    apply(&mut doc, side_by_side, depth, popout);
    crate::fsutil::write_json_atomic(path, &doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_expected_section_and_keeps_other_content() {
        let mut doc =
            json!({"Other": {"x": 1}, "StandardVideo": {"3D Depth": "0.100000", "Custom": true}});
        apply(&mut doc, false, 0.9, 0.0);
        assert_eq!(doc["Other"]["x"], 1);
        assert_eq!(doc["StandardVideo"]["Custom"], true);
        assert_eq!(doc["StandardVideo"]["3D Depth"], "0.900000");
        assert_eq!(doc["StandardVideo"]["3D Popout"], "0.000000");
        assert_eq!(doc["StandardVideo"]["3D Off"], false);
        assert_eq!(doc["StandardVideo"]["Overlay Off"], true);
        assert!(doc.get("SideBySideVideo").is_none());
        apply(&mut doc, true, 1.25, -0.5);
        assert_eq!(doc["SideBySideVideo"]["3D Depth"], "1.250000");
        assert_eq!(doc["SideBySideVideo"]["3D Popout"], "-0.500000");
    }

    #[test]
    fn non_object_document_is_replaced() {
        let mut doc = json!([1, 2, 3]);
        apply(&mut doc, false, 0.9, 0.0);
        assert!(doc["StandardVideo"].is_object());
    }

    #[test]
    fn malformed_settings_are_preserved_instead_of_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("OverlaySettings.json");
        let backup = dir.path().join("backup.json");
        for original in [
            r#"{"Other": "incomplete""#,
            "[]",
            "null",
            r#"{"StandardVideo":42}"#,
        ] {
            std::fs::write(&path, original).unwrap();
            assert!(update_file(&path, &backup, false, 0.9, 0.0).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert!(
                !backup.exists(),
                "invalid input must not occupy the one-time backup"
            );
        }
    }

    #[test]
    fn unreadable_input_does_not_create_a_backup_or_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("OverlaySettings.json");
        let backup = dir.path().join("backup.json");
        std::fs::create_dir(&path).unwrap();
        assert!(update_file(&path, &backup, false, 0.9, 0.0).is_err());
        assert!(path.is_dir());
        assert!(!backup.exists());
    }

    #[test]
    fn file_update_backs_up_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("OverlaySettings.json");
        let backup = dir.path().join("data").join("backup.json");
        std::fs::write(&path, r#"{"StandardVideo":{"3D Depth":"0.500000"}}"#).unwrap();
        update_file(&path, &backup, false, 0.9, 0.1).unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            r#"{"StandardVideo":{"3D Depth":"0.500000"}}"#
        );
        update_file(&path, &backup, false, 1.0, 0.1).unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            r#"{"StandardVideo":{"3D Depth":"0.500000"}}"#,
            "backup is never overwritten"
        );
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["StandardVideo"]["3D Depth"], "1.000000");
        // Missing Samsung file: no backup, file created.
        let missing = dir.path().join("none.json");
        update_file(&missing, &dir.path().join("b2.json"), true, 0.9, 0.0).unwrap();
        assert!(missing.is_file() && !dir.path().join("b2.json").exists());
    }
}
