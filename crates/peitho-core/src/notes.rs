use std::collections::BTreeMap;

use serde::Serialize;

use crate::{domain::SlideKey, error::Result, json::pretty_json};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notes {
    version: u8,
    #[cfg_attr(
        any(test, feature = "ts-bindings"),
        ts(type = "Record<string, string>")
    )]
    notes: BTreeMap<SlideKey, String>,
}

impl Notes {
    pub fn empty() -> Self {
        Self {
            version: 1,
            notes: BTreeMap::new(),
        }
    }

    pub fn new(notes: BTreeMap<SlideKey, String>) -> Self {
        Self { version: 1, notes }
    }
}

pub fn notes_json(notes: &Notes) -> Result<String> {
    pretty_json(notes, "notes", "keep notes fields serializable")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use ts_rs::{Config, TS};

    use super::Notes;
    use crate::domain::SlideKey;

    #[test]
    fn serializes_empty_notes_schema() {
        let notes = Notes::empty();
        let json = serde_json::to_string_pretty(&notes).unwrap();

        assert!(json.contains(r#""version": 1"#));
        assert!(json.contains(r#""notes": {}"#));
    }

    #[test]
    fn serializes_notes_json_with_trailing_newline() {
        let json = super::notes_json(&Notes::empty()).unwrap();

        assert_eq!(json, "{\n  \"version\": 1,\n  \"notes\": {}\n}\n");
    }

    #[test]
    fn exports_notes_binding_as_keyed_record() {
        let cfg = Config::from_env();
        Notes::export_all(&cfg).unwrap();

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/Notes.ts");
        let ts = fs::read_to_string(path).unwrap();
        assert!(ts.contains("notes: Record<string, string>"));
    }

    #[test]
    fn notes_are_keyed_by_slide_key_not_index() {
        let mut map = BTreeMap::new();
        map.insert(SlideKey::new("arch-1").unwrap(), "speaker note".to_owned());
        let notes = Notes::new(map);
        let json = serde_json::to_string(&notes).unwrap();

        assert!(json.contains(r#""arch-1":"speaker note""#));
    }
}
