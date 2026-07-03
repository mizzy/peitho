use serde::Serialize;

use crate::error::{BuildError, ErrorKind, Result};

pub(crate) fn pretty_json<T: Serialize>(value: &T, what: &str, help: &str) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value).map_err(|err| {
        BuildError::new(
            ErrorKind::Manifest,
            None,
            format!("failed to serialize {what}: {err}"),
            help,
        )
    })?;
    json.push('\n');
    Ok(json)
}
