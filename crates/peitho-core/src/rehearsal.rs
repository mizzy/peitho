use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::{domain::SlideKey, error::Result, json::pretty_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RehearsalVersion1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RehearsalVersion2;

macro_rules! rehearsal_version {
    ($type:ty, $version:literal) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_u8($version)
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let version = u64::deserialize(deserializer)?;
                if version == $version {
                    Ok(Self)
                } else {
                    Err(de::Error::custom(format_args!(
                        "unsupported rehearsal version {version}"
                    )))
                }
            }
        }
    };
}

rehearsal_version!(RehearsalVersion1, 1);
rehearsal_version!(RehearsalVersion2, 2);

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
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "2"))]
    version: RehearsalVersion2,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
    timeline: Vec<RehearsalSlideEntry>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RehearsalSlideEntry {
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
    key: SlideKey,
    index: u32,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    at_ms: u64,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RehearsalRecordV1 {
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "1"))]
    version: RehearsalVersion1,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    recorded_at_ms: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawRehearsalRecordV1 {
    version: RehearsalVersion1,
    recorded_at_ms: u64,
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RehearsalRecordV2 {
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "2"))]
    version: RehearsalVersion2,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    recorded_at_ms: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
    timeline: Vec<RehearsalSlideEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawRehearsalRecordV2 {
    version: RehearsalVersion2,
    recorded_at_ms: u64,
    elapsed_ms: u64,
    sections: Vec<RehearsalSection>,
    timeline: Vec<RehearsalSlideEntry>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RehearsalRecord {
    V1(RehearsalRecordV1),
    V2(RehearsalRecordV2),
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

impl RehearsalSlideEntry {
    pub fn key(&self) -> &SlideKey {
        &self.key
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn at_ms(&self) -> u64 {
        self.at_ms
    }
}

impl RehearsalSnapshot {
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    pub fn sections(&self) -> &[RehearsalSection] {
        &self.sections
    }

    pub fn timeline(&self) -> &[RehearsalSlideEntry] {
        &self.timeline
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        validate_nonempty_sections(&self.sections)
    }

    pub fn validate_timeline(&self) -> std::result::Result<(), String> {
        validate_timeline_shape(&self.timeline, self.elapsed_ms)
    }
}

impl RehearsalRecordV1 {
    pub fn new(
        recorded_at_ms: u64,
        elapsed_ms: u64,
        sections: Vec<RehearsalSection>,
    ) -> std::result::Result<Self, String> {
        Self::try_from(RawRehearsalRecordV1 {
            version: RehearsalVersion1,
            recorded_at_ms,
            elapsed_ms,
            sections,
        })
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
}

impl TryFrom<RawRehearsalRecordV1> for RehearsalRecordV1 {
    type Error = String;

    fn try_from(raw: RawRehearsalRecordV1) -> std::result::Result<Self, Self::Error> {
        validate_nonempty_sections(&raw.sections)?;
        Ok(Self {
            version: raw.version,
            recorded_at_ms: raw.recorded_at_ms,
            elapsed_ms: raw.elapsed_ms,
            sections: raw.sections,
        })
    }
}

impl<'de> Deserialize<'de> for RehearsalRecordV1 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::try_from(RawRehearsalRecordV1::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl RehearsalRecordV2 {
    pub fn from_snapshot(
        recorded_at_ms: u64,
        snapshot: &RehearsalSnapshot,
    ) -> std::result::Result<Self, String> {
        Self::try_from(RawRehearsalRecordV2 {
            version: RehearsalVersion2,
            recorded_at_ms,
            elapsed_ms: snapshot.elapsed_ms(),
            sections: snapshot.sections().to_vec(),
            timeline: snapshot.timeline().to_vec(),
        })
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

    pub fn timeline(&self) -> &[RehearsalSlideEntry] {
        &self.timeline
    }
}

impl TryFrom<RawRehearsalRecordV2> for RehearsalRecordV2 {
    type Error = String;

    fn try_from(raw: RawRehearsalRecordV2) -> std::result::Result<Self, Self::Error> {
        validate_nonempty_sections(&raw.sections)?;
        validate_timeline_shape(&raw.timeline, raw.elapsed_ms)?;
        Ok(Self {
            version: raw.version,
            recorded_at_ms: raw.recorded_at_ms,
            elapsed_ms: raw.elapsed_ms,
            sections: raw.sections,
            timeline: raw.timeline,
        })
    }
}

impl<'de> Deserialize<'de> for RehearsalRecordV2 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::try_from(RawRehearsalRecordV2::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl RehearsalRecord {
    pub fn recorded_at_ms(&self) -> u64 {
        match self {
            Self::V1(record) => record.recorded_at_ms(),
            Self::V2(record) => record.recorded_at_ms(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        match self {
            Self::V1(record) => record.elapsed_ms(),
            Self::V2(record) => record.elapsed_ms(),
        }
    }

    pub fn sections(&self) -> &[RehearsalSection] {
        match self {
            Self::V1(record) => record.sections(),
            Self::V2(record) => record.sections(),
        }
    }
}

impl<'de> Deserialize<'de> for RehearsalRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| de::Error::custom("rehearsal version must be an unsigned integer"))?;
        match version {
            1 => serde_json::from_value(value)
                .map(Self::V1)
                .map_err(de::Error::custom),
            2 => serde_json::from_value(value)
                .map(Self::V2)
                .map_err(de::Error::custom),
            _ => Err(de::Error::custom(format_args!(
                "unsupported rehearsal version {version}"
            ))),
        }
    }
}

fn validate_nonempty_sections(sections: &[RehearsalSection]) -> std::result::Result<(), String> {
    if sections.is_empty() {
        Err("rehearsal sections must not be empty".to_owned())
    } else {
        Ok(())
    }
}

fn validate_timeline_shape(
    timeline: &[RehearsalSlideEntry],
    elapsed_ms: u64,
) -> std::result::Result<(), String> {
    let mut previous_at_ms = None;
    for entry in timeline {
        if previous_at_ms.is_some_and(|previous| entry.at_ms() < previous) {
            return Err("rehearsal timeline positions must be non-decreasing".to_owned());
        }
        if entry.at_ms() > elapsed_ms {
            return Err(format!(
                "rehearsal timeline position {} exceeds elapsed time {elapsed_ms}",
                entry.at_ms()
            ));
        }
        previous_at_ms = Some(entry.at_ms());
    }
    Ok(())
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

    use super::{
        RehearsalRecord, RehearsalRecordV1, RehearsalRecordV2, RehearsalSection,
        RehearsalSlideEntry, RehearsalSnapshot,
    };

    fn snapshot(json: &str) -> RehearsalSnapshot {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn exports_rehearsal_bindings() {
        let cfg = Config::from_env();
        RehearsalSection::export_all(&cfg).unwrap();
        RehearsalSlideEntry::export_all(&cfg).unwrap();
        RehearsalSnapshot::export_all(&cfg).unwrap();
        RehearsalRecordV1::export_all(&cfg).unwrap();
        RehearsalRecordV2::export_all(&cfg).unwrap();
        RehearsalRecord::export_all(&cfg).unwrap();

        let bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let snapshot = fs::read_to_string(bindings.join("RehearsalSnapshot.ts")).unwrap();
        let v1 = fs::read_to_string(bindings.join("RehearsalRecordV1.ts")).unwrap();
        let v2 = fs::read_to_string(bindings.join("RehearsalRecordV2.ts")).unwrap();
        let record = fs::read_to_string(bindings.join("RehearsalRecord.ts")).unwrap();

        assert!(snapshot.contains("version: 2"));
        assert!(snapshot.contains("timeline: Array<RehearsalSlideEntry>"));
        assert!(v1.contains("version: 1"));
        assert!(!v1.contains("timeline"));
        assert!(v2.contains("version: 2"));
        assert!(v2.contains("timeline: Array<RehearsalSlideEntry>"));
        assert!(record.contains("RehearsalRecordV1 | RehearsalRecordV2"));
    }

    #[test]
    fn decodes_v1_and_v2_records_at_the_enum_boundary() {
        let v1: RehearsalRecord = serde_json::from_str(
            r#"{"version":1,"recordedAtMs":10,"elapsedMs":9000,
                 "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":9000}]}"#,
        )
        .unwrap();
        assert!(matches!(v1, RehearsalRecord::V1(_)));

        let v2: RehearsalRecord = serde_json::from_str(
            r#"{"version":2,"recordedAtMs":10,"elapsedMs":9000,
                 "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":9000}],
                 "timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        )
        .unwrap();
        let RehearsalRecord::V2(v2) = v2 else {
            panic!("expected v2");
        };
        assert_eq!(v2.timeline()[0].key().as_str(), "intro");
    }

    #[test]
    fn accepts_only_v2_snapshots() {
        let valid = snapshot(
            r#"{"version":2,"elapsedMs":9000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":9000}],
                "timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        );
        assert_eq!(valid.timeline()[0].index(), 0);
        assert!(valid.validate().is_ok());

        let err = serde_json::from_str::<RehearsalSnapshot>(
            r#"{"version":1,"elapsedMs":0,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":0}],
                "timeline":[]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported rehearsal version 1"));
    }

    #[test]
    fn reports_unknown_record_versions_by_number() {
        let err = serde_json::from_str::<RehearsalRecord>(
            r#"{"version":3,"recordedAtMs":10,"elapsedMs":0,"sections":[],"timeline":[]}"#,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "unsupported rehearsal version 3");

        for json in [
            r#"{"recordedAtMs":10,"elapsedMs":0,"sections":[]}"#,
            r#"{"version":"2","recordedAtMs":10,"elapsedMs":0,"sections":[]}"#,
        ] {
            assert!(serde_json::from_str::<RehearsalRecord>(json)
                .unwrap_err()
                .to_string()
                .contains("rehearsal version must be an unsigned integer"));
        }
    }

    #[test]
    fn rejects_unknown_v1_record_fields() {
        let v1_with_unknown = r#"{"version":1,"recordedAtMs":10,"elapsedMs":0,
            "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":0}],
            "extra":true}"#;
        assert!(serde_json::from_str::<RehearsalRecord>(v1_with_unknown).is_err());
    }

    #[test]
    fn keeps_timeline_shape_validation_out_of_snapshot_schema_validation() {
        let decreasing = snapshot(
            r#"{"version":2,"elapsedMs":1000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],
                "timeline":[{"key":"intro","index":0,"atMs":500},
                            {"key":"details","index":1,"atMs":499}]}"#,
        );
        assert!(decreasing.validate().is_ok());
        assert_eq!(
            decreasing.validate_timeline().unwrap_err(),
            "rehearsal timeline positions must be non-decreasing"
        );

        let after_elapsed = snapshot(
            r#"{"version":2,"elapsedMs":1000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],
                "timeline":[{"key":"intro","index":0,"atMs":1001}]}"#,
        );
        assert_eq!(
            after_elapsed.validate_timeline().unwrap_err(),
            "rehearsal timeline position 1001 exceeds elapsed time 1000"
        );
    }

    #[test]
    fn validates_v2_record_timeline_shape() {
        let valid_snapshot = snapshot(
            r#"{"version":2,"elapsedMs":1000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],
                "timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        );
        let record =
            RehearsalRecord::V2(RehearsalRecordV2::from_snapshot(10, &valid_snapshot).unwrap());
        assert_eq!(record.recorded_at_ms(), 10);
        assert_eq!(record.elapsed_ms(), 1_000);
        assert_eq!(record.sections()[0].name(), "Setup");

        let invalid = snapshot(
            r#"{"version":2,"elapsedMs":1000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],
                "timeline":[{"key":"intro","index":0,"atMs":500},
                            {"key":"details","index":1,"atMs":499}]}"#,
        );
        assert_eq!(
            RehearsalRecordV2::from_snapshot(10, &invalid).unwrap_err(),
            "rehearsal timeline positions must be non-decreasing"
        );

        let empty = snapshot(r#"{"version":2,"elapsedMs":0,"sections":[],"timeline":[]}"#);
        assert_eq!(
            RehearsalRecordV2::from_snapshot(10, &empty).unwrap_err(),
            "rehearsal sections must not be empty"
        );
    }

    #[test]
    fn v2_deserialization_enforces_construction_invariants() {
        let empty_sections = serde_json::from_str::<RehearsalRecordV2>(
            r#"{"version":2,"recordedAtMs":10,"elapsedMs":0,"sections":[],"timeline":[]}"#,
        )
        .unwrap_err();
        assert!(empty_sections
            .to_string()
            .contains("rehearsal sections must not be empty"));

        let decreasing = serde_json::from_str::<RehearsalRecordV2>(
            r#"{"version":2,"recordedAtMs":10,"elapsedMs":1000,
                "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],
                "timeline":[{"key":"intro","index":0,"atMs":500},
                            {"key":"details","index":1,"atMs":499}]}"#,
        )
        .unwrap_err();
        assert!(decreasing
            .to_string()
            .contains("rehearsal timeline positions must be non-decreasing"));
    }

    #[test]
    fn v1_deserialization_enforces_construction_invariants() {
        let empty_sections = serde_json::from_str::<RehearsalRecordV1>(
            r#"{"version":1,"recordedAtMs":10,"elapsedMs":0,"sections":[]}"#,
        )
        .unwrap_err();

        assert!(empty_sections
            .to_string()
            .contains("rehearsal sections must not be empty"));

        let empty_record = serde_json::from_str::<RehearsalRecord>(
            r#"{"version":1,"recordedAtMs":10,"elapsedMs":0,"sections":[]}"#,
        )
        .unwrap_err();
        assert!(empty_record
            .to_string()
            .contains("rehearsal sections must not be empty"));
    }

    #[test]
    fn v1_constructor_enforces_construction_invariants() {
        assert_eq!(
            RehearsalRecordV1::new(10, 0, Vec::new()).unwrap_err(),
            "rehearsal sections must not be empty"
        );
    }
}
