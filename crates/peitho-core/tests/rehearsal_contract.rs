use peitho_core::{
    rehearsal_baseline_json, rehearsal_record_json, RehearsalBaseline, RehearsalRecord,
    RehearsalSection, RehearsalSnapshot,
};

#[test]
fn deserializes_rehearsal_snapshot_wire_schema() {
    let snapshot: RehearsalSnapshot = serde_json::from_str(
        r#"{"version":1,"elapsedMs":12345,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":52000},{"name":"Demo","plannedDurationMs":120000,"actualMs":133000}]}"#,
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
}

#[test]
fn serializes_rehearsal_record_schema_exactly() {
    let record = RehearsalRecord::new(
        1_783_000_000_123,
        12_345,
        vec![RehearsalSection::new("Setup", 60_000, 52_000)],
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
fn serializes_rehearsal_baseline_with_null_last_run() {
    let json = rehearsal_baseline_json(&RehearsalBaseline::empty()).unwrap();

    assert_eq!(json, "{\n  \"version\": 1,\n  \"lastRun\": null\n}\n");
}

#[test]
fn serializes_rehearsal_baseline_with_record() {
    let baseline = RehearsalBaseline::new(Some(RehearsalRecord::new(
        1_783_000_000_123,
        12_345,
        vec![RehearsalSection::new("Setup", 60_000, 52_000)],
    )));

    let json = rehearsal_baseline_json(&baseline).unwrap();

    assert!(json.contains(r#""version": 1"#));
    assert!(json.contains(r#""lastRun": {"#));
    assert!(json.contains(r#""recordedAtMs": 1783000000123"#));
}

#[test]
fn validates_rehearsal_snapshot_version_and_sections() {
    let future: RehearsalSnapshot =
        serde_json::from_str(r#"{"version":2,"elapsedMs":0,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":0}]}"#)
            .unwrap();
    let empty: RehearsalSnapshot =
        serde_json::from_str(r#"{"version":1,"elapsedMs":0,"sections":[]}"#).unwrap();

    assert!(future.validate().unwrap_err().contains("version"));
    assert!(empty.validate().unwrap_err().contains("sections"));
}

#[test]
fn rejects_unknown_rehearsal_snapshot_fields() {
    assert!(serde_json::from_str::<RehearsalSnapshot>(
        r#"{"version":1,"elapsedMs":0,"sections":[],"extra":true}"#
    )
    .is_err());
}
