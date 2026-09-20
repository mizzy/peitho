use serde::Serialize;
use serde_json::Value;

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTimerSnapshot {
    running: bool,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    elapsed_ms: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    at_ms: u64,
}

impl SyncTimerSnapshot {
    pub fn new(running: bool, elapsed_ms: u64, at_ms: u64) -> Self {
        Self {
            running,
            elapsed_ms,
            at_ms,
        }
    }

    pub fn running(self) -> bool {
        self.running
    }

    pub fn elapsed_ms(self) -> u64 {
        self.elapsed_ms
    }

    pub fn at_ms(self) -> u64 {
        self.at_ms
    }
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    seq: u64,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "unknown"))]
    message: Option<Value>,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number | null"))]
    index: Option<usize>,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number | null"))]
    step: Option<usize>,
    swapped: bool,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    generation: u64,
    session: String,
    timer: Option<SyncTimerSnapshot>,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    now_ms: u64,
    build_error: Option<String>,
}

impl SyncResponse {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seq: u64,
        message: Option<Value>,
        index: Option<usize>,
        step: Option<usize>,
        swapped: bool,
        generation: u64,
        session: String,
        timer: Option<SyncTimerSnapshot>,
        now_ms: u64,
        build_error: Option<String>,
    ) -> Self {
        Self {
            seq,
            message,
            index,
            step,
            swapped,
            generation,
            session,
            timer,
            now_ms,
            build_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use serde_json::json;
    use ts_rs::{Config, TS};

    use super::{SyncResponse, SyncTimerSnapshot};

    #[test]
    fn serializes_full_sync_response_without_changing_existing_wire_fields() {
        let response = SyncResponse::new(
            7,
            Some(json!({"index": 2, "step": 3})),
            Some(2),
            Some(3),
            true,
            9,
            "session-a".to_owned(),
            Some(SyncTimerSnapshot::new(true, 12_000, 98_000)),
            100_000,
            None,
        );

        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"seq":7,"message":{"index":2,"step":3},"index":2,"step":3,"swapped":true,"generation":9,"session":"session-a","timer":{"running":true,"elapsedMs":12000,"atMs":98000},"nowMs":100000,"buildError":null}"#
        );
    }

    #[test]
    fn exports_sync_response_and_timer_snapshot_bindings() {
        let cfg = Config::from_env();
        SyncTimerSnapshot::export_all(&cfg).unwrap();
        SyncResponse::export_all(&cfg).unwrap();

        let bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let response = fs::read_to_string(bindings.join("SyncResponse.ts")).unwrap();
        let timer = fs::read_to_string(bindings.join("SyncTimerSnapshot.ts")).unwrap();

        assert!(response.contains("seq: number"));
        assert!(response.contains("message: unknown"));
        assert!(response.contains("index: number | null"));
        assert!(response.contains("step: number | null"));
        assert!(response.contains("timer: SyncTimerSnapshot | null"));
        assert!(response.contains("nowMs: number"));
        assert!(response.contains("buildError: string | null"));
        assert!(timer.contains("elapsedMs: number"));
        assert!(timer.contains("atMs: number"));
    }
}
