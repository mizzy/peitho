use serde::{Deserialize, Serialize};

use crate::{error::Result, json::pretty_json};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentConfig {
    version: u8,
    #[serde(rename = "presenterOpen")]
    presenter_open: bool,
}

impl PresentConfig {
    pub fn new(presenter_open: bool) -> Self {
        Self {
            version: 1,
            presenter_open,
        }
    }

    pub fn presenter_open(&self) -> bool {
        self.presenter_open
    }
}

pub fn present_config_json(config: &PresentConfig) -> Result<String> {
    pretty_json(
        config,
        "present config",
        "keep present config fields serializable",
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use ts_rs::{Config, TS};

    use super::{present_config_json, PresentConfig};

    #[test]
    fn serializes_present_config_schema_exactly() {
        let json = present_config_json(&PresentConfig::new(true)).unwrap();

        assert_eq!(json, "{\n  \"version\": 1,\n  \"presenterOpen\": true\n}\n");
    }

    #[test]
    fn exports_present_config_binding_with_serde_field_names() {
        let cfg = Config::from_env();
        PresentConfig::export_all(&cfg).unwrap();

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/PresentConfig.ts");
        let ts = fs::read_to_string(path).unwrap();

        assert!(ts.contains("version: number"));
        assert!(ts.contains("presenterOpen: boolean"));
    }
}
