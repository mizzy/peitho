use serde::{Deserialize, Serialize};

use crate::{error::Result, json::pretty_json};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RehearsalSection {
    name: String,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    planned_duration_ms: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    actual_ms: u64,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RehearsalSnapshot {
    version: u8,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RehearsalRecord {
    version: u8,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    recorded_at_ms: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
}

impl RehearsalSection {
    pub fn new(name: impl Into<String>, planned_duration_ms: u64, actual_ms: u64) -> Self {
        Self {
            name: name.into(),
            planned_duration_ms,
            actual_ms,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn planned_duration_ms(&self) -> u64 {
        self.planned_duration_ms
    }

    pub fn actual_ms(&self) -> u64 {
        self.actual_ms
    }
}

impl RehearsalSnapshot {
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    pub fn sections(&self) -> &[RehearsalSection] {
        &self.sections
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        validate_version(self.version)?;
        validate_nonempty_sections(&self.sections)
    }
}

impl RehearsalRecord {
    pub fn new(recorded_at_ms: u64, elapsed_ms: u64, sections: Vec<RehearsalSection>) -> Self {
        Self {
            version: 1,
            recorded_at_ms,
            elapsed_ms,
            sections,
        }
    }

    pub fn from_snapshot(recorded_at_ms: u64, snapshot: &RehearsalSnapshot) -> Self {
        Self::new(
            recorded_at_ms,
            snapshot.elapsed_ms(),
            snapshot.sections().to_vec(),
        )
    }

    pub fn recorded_at_ms(&self) -> u64 {
        self.recorded_at_ms
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    pub fn sections(&self) -> &[RehearsalSection] {
        &self.sections
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        validate_version(self.version)?;
        validate_nonempty_sections(&self.sections)
    }
}

fn validate_version(version: u8) -> std::result::Result<(), String> {
    if version == 1 {
        Ok(())
    } else {
        Err(format!("unsupported rehearsal version {version}"))
    }
}

fn validate_nonempty_sections(sections: &[RehearsalSection]) -> std::result::Result<(), String> {
    if sections.is_empty() {
        Err("rehearsal sections must not be empty".to_owned())
    } else {
        Ok(())
    }
}

pub fn rehearsal_record_json(record: &RehearsalRecord) -> Result<String> {
    pretty_json(
        record,
        "rehearsal record",
        "keep rehearsal record fields serializable",
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use ts_rs::{Config, TS};

    use super::{RehearsalRecord, RehearsalSection, RehearsalSnapshot};

    #[test]
    fn exports_rehearsal_bindings() {
        let cfg = Config::from_env();
        RehearsalSection::export_all(&cfg).unwrap();
        RehearsalSnapshot::export_all(&cfg).unwrap();
        RehearsalRecord::export_all(&cfg).unwrap();

        let bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let section = fs::read_to_string(bindings.join("RehearsalSection.ts")).unwrap();
        let snapshot = fs::read_to_string(bindings.join("RehearsalSnapshot.ts")).unwrap();
        let record = fs::read_to_string(bindings.join("RehearsalRecord.ts")).unwrap();

        assert!(section.contains("plannedDurationMs: number"));
        assert!(section.contains("actualMs: number"));
        assert!(snapshot.contains("elapsedMs: number"));
        assert!(record.contains("recordedAtMs: number"));
    }
}
