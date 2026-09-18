use peitho_core::{
    rehearsal_record_json, RehearsalRecord, RehearsalRecordV1, RehearsalSection, RehearsalSnapshot,
};

#[test]
fn deserializes_rehearsal_snapshot_wire_schema() {
    let snapshot: RehearsalSnapshot = serde_json::from_str(
        r#"{"version":2,"elapsedMs":12345,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":52000},{"name":"Demo","plannedDurationMs":120000,"actualMs":133000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
    )
    .unwrap();

    snapshot.validate().unwrap();
    assert_eq!(snapshot.elapsed_ms(), 12_345);
    assert_eq!(snapshot.sections()[0].name(), "Setup");
    assert_eq!(snapshot.sections()[0].planned_duration_ms(), 60_000);
    assert_eq!(snapshot.sections()[0].actual_ms(), 52_000);
    assert_eq!(snapshot.sections()[1].name(), "Demo");
    assert_eq!(snapshot.sections()[1].planned_duration_ms(), 120_000);
    assert_eq!(snapshot.sections()[1].actual_ms(), 133_000);
    assert_eq!(snapshot.timeline()[0].key().as_str(), "intro");
}

#[test]
fn serializes_rehearsal_record_schema_exactly() {
    let record = RehearsalRecord::V1(
        RehearsalRecordV1::new(
            1_783_000_000_123,
            12_345,
            vec![RehearsalSection::new("Setup", 60_000, 52_000)],
        )
        .unwrap(),
    );

    let json = rehearsal_record_json(&record).unwrap();

    assert_eq!(
        json,
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"recordedAtMs\": 1783000000123,\n",
            "  \"elapsedMs\": 12345,\n",
            "  \"sections\": [\n",
            "    {\n",
            "      \"name\": \"Setup\",\n",
            "      \"plannedDurationMs\": 60000,\n",
            "      \"actualMs\": 52000\n",
            "    }\n",
            "  ]\n",
            "}\n"
        )
    );
}

#[test]
fn validates_rehearsal_snapshot_version_and_sections() {
    let future = serde_json::from_str::<RehearsalSnapshot>(
        r#"{"version":3,"elapsedMs":0,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":0}],"timeline":[]}"#,
    )
    .unwrap_err();
    let empty: RehearsalSnapshot =
        serde_json::from_str(r#"{"version":2,"elapsedMs":0,"sections":[],"timeline":[]}"#).unwrap();

    assert!(future
        .to_string()
        .contains("unsupported rehearsal version 3"));
    assert!(empty.validate().unwrap_err().contains("sections"));
}

#[test]
fn rejects_unknown_rehearsal_snapshot_fields() {
    assert!(serde_json::from_str::<RehearsalSnapshot>(
        r#"{"version":2,"elapsedMs":0,"sections":[],"timeline":[],"extra":true}"#
    )
    .is_err());
}
