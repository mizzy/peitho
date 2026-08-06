use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use miette::IntoDiagnostic;
use serde::Deserialize;

pub(crate) const PEITHO_LINT_DONE: &str = "PEITHO_LINT_DONE";
const PEITHO_LINT_CHUNK: &str = "PEITHO_LINT_CHUNK";
const MIN_FONT_SIZE_PT: f64 = 24.0;
const OVERFLOW_TOLERANCE_PX: i64 = 1;
const OVERFLOW_HELP: &str = "shrink or split the slide content, or adjust the layout CSS";
const SCROLLABLE_OVERFLOW_HELP: &str =
    "a scrollable region cannot be scrolled in a printed or projected deck, so content past the edge will not be seen";
const FONT_SIZE_HELP: &str =
    "raise the font size in the layout CSS, or move content to another slide instead of shrinking it";
const LINT_PARSE_HELP: &str =
    "rerun lint and inspect lint.html and chrome-stderr.log in the kept workspace";

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct SlideMeasurement {
    slide: usize,
    #[serde(rename = "contentWidth")]
    content_width: f64,
    #[serde(rename = "contentHeight")]
    content_height: f64,
    #[serde(rename = "boxWidth")]
    box_width: f64,
    #[serde(rename = "boxHeight")]
    box_height: f64,
    #[serde(rename = "minFontSizePx")]
    min_font_size_px: Option<f64>,
    #[serde(rename = "minFontSample")]
    min_font_sample: Option<String>,
    #[serde(default, rename = "slotOverflows")]
    slot_overflows: Vec<SlotOverflowMeasurement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SlotOverflowMeasurement {
    #[serde(rename = "slotOverflowAxis")]
    axis: Option<OverflowAxis>,
    #[serde(rename = "slotOverflowPx")]
    overflow_px: Option<i64>,
    #[serde(rename = "slotOverflowValue")]
    overflow_value: Option<OverflowValue>,
    #[serde(rename = "slotName")]
    slot: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OverflowAxis {
    Horizontal,
    Vertical,
}

impl OverflowAxis {
    fn adverb(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontally",
            Self::Vertical => "vertically",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OverflowValue {
    Hidden,
    Clip,
    Auto,
    Scroll,
}

impl OverflowValue {
    fn help(self) -> &'static str {
        match self {
            Self::Hidden | Self::Clip => OVERFLOW_HELP,
            Self::Auto | Self::Scroll => SCROLLABLE_OVERFLOW_HELP,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverflowWarning {
    slide: usize,
    axis: OverflowAxis,
    overflow_px: i64,
    content_px: i64,
    box_px: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlotOverflowWarning {
    slide: usize,
    axis: OverflowAxis,
    overflow_px: i64,
    overflow_value: Option<OverflowValue>,
    slot: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct FontSizeWarning {
    slide: usize,
    font_size_pt: f64,
    sample: String,
}

pub(crate) fn run(input: PathBuf, stdout: &mut dyn Write) -> miette::Result<i32> {
    let artifacts = crate::build_artifacts(&input)?;
    let tmp = tempfile::tempdir().into_diagnostic()?;
    emit_lint_workspace(tmp.path(), &artifacts)?;
    let chrome = crate::locate_chrome()?;
    let chrome_log = match run_chrome_lint(&chrome, tmp.path()) {
        Ok(stderr) => stderr,
        Err(err) => return Err(crate::keep_workspace_for_error(tmp, err)),
    };
    let chrome_log = String::from_utf8_lossy(&chrome_log);
    let measurements = match parse_lint_measurements(&chrome_log, artifacts.slide_count) {
        Ok(measurements) => measurements,
        Err(err) => {
            if let Err(write_err) = write_chrome_stderr_log(tmp.path(), &chrome_log) {
                return Err(crate::keep_workspace_for_error(
                    tmp,
                    append_chrome_stderr_log_write_failure(err, write_err),
                ));
            }
            return Err(crate::keep_workspace_for_error(tmp, err));
        }
    };
    write_lint_report(&measurements, stdout)
}

fn write_chrome_stderr_log(workspace: &Path, chrome_log: &str) -> miette::Result<()> {
    let log_path = workspace.join("chrome-stderr.log");
    fs::write(&log_path, chrome_log).map_err(|err| {
        miette::miette!(
            help = "rerun lint and inspect lint.html in the kept workspace",
            "failed to write Chrome stderr log to {}\ncaused by: {err}",
            log_path.display()
        )
    })
}

fn append_chrome_stderr_log_write_failure(
    parse_error: miette::Report,
    write_error: miette::Report,
) -> miette::Report {
    let mut help_parts: Vec<String> = Vec::new();
    for help in [
        crate::diagnostics::report_help(&parse_error),
        crate::diagnostics::report_help(&write_error),
    ]
    .into_iter()
    .flatten()
    {
        if !help_parts.contains(&help) {
            help_parts.push(help);
        }
    }
    let message = format!(
        "{}\nnote: failed to write chrome-stderr.log: {}",
        parse_error, write_error
    );
    if help_parts.is_empty() {
        miette::miette!("{message}")
    } else {
        miette::miette!(help = help_parts.join("\n"), "{message}")
    }
}

fn emit_lint_workspace(workspace: &Path, artifacts: &crate::BuildArtifacts) -> miette::Result<()> {
    crate::write_shared_assets(workspace, artifacts)?;
    let lint_html = peitho_core::render_lint_document(&artifacts.rendered);
    fs::write(workspace.join("lint.html"), lint_html).into_diagnostic()?;
    Ok(())
}

fn run_chrome_lint(chrome: &Path, workspace: &Path) -> miette::Result<Vec<u8>> {
    let profile = workspace.join("chrome-profile");
    fs::create_dir_all(&profile).into_diagnostic()?;
    let lint_html = workspace.join("lint.html");
    let lint_pdf = workspace.join("lint.pdf");
    let url = crate::file_url(&lint_html)?;
    let args = crate::chrome_print_args(&profile, &lint_pdf, &url);
    let output = crate::run_one_shot_chrome(
        chrome,
        &args,
        crate::ChromeCompletion::LintResultLogged,
        crate::CHROME_ONE_SHOT_TIMEOUT,
    )?;
    Ok(output.stderr)
}

fn parse_lint_measurements(
    chrome_log: &str,
    expected_slide_count: usize,
) -> miette::Result<Vec<SlideMeasurement>> {
    let payload = extract_lint_payload(chrome_log)?;
    let json = STANDARD.decode(payload).map_err(|err| {
        miette::miette!(
            help = LINT_PARSE_HELP,
            "lint measurement payload is not valid base64\ncaused by: {err}"
        )
    })?;
    let measurements: Vec<SlideMeasurement> = serde_json::from_slice(&json).map_err(|err| {
        miette::miette!(
            help = LINT_PARSE_HELP,
            "lint measurement payload is not valid JSON\ncaused by: {err}"
        )
    })?;
    if measurements.len() != expected_slide_count {
        return Err(miette::miette!(
            help = format!("no lint result was accepted; {LINT_PARSE_HELP}"),
            "lint measurement slide count mismatch: expected {expected_slide_count}, got {}",
            measurements.len()
        ));
    }
    Ok(measurements)
}

#[derive(Debug)]
struct LintPayloadChunk<'a> {
    index: usize,
    total: usize,
    slice: &'a str,
    line: usize,
}

fn extract_lint_payload(chrome_log: &str) -> miette::Result<String> {
    let chunks = lint_payload_chunks(chrome_log)?;
    if chunks.is_empty() {
        return Err(lint_parse_error(
            "no lint measurement chunks found in Chrome log".to_owned(),
        ));
    }

    let expected_total = chunks[0].total;
    if expected_total == 0 {
        return Err(lint_parse_error(format!(
            "inconsistent lint measurement chunk totals at line {}: total must be greater than zero",
            chunks[0].line
        )));
    }
    if expected_total > chunks.len() {
        let missing_index = first_missing_chunk_index(&chunks);
        return Err(lint_parse_error(format!(
            "missing lint measurement chunk index {missing_index}"
        )));
    }

    let mut slices = vec![None; expected_total];
    for chunk in chunks {
        if chunk.total != expected_total {
            return Err(lint_parse_error(format!(
                "inconsistent lint measurement chunk totals at line {}: expected {expected_total}, got {}",
                chunk.line, chunk.total
            )));
        }
        if chunk.index == 0 || chunk.index > expected_total {
            return Err(lint_parse_error(format!(
                "missing lint measurement chunk index {} at line {}: expected indexes 1..={expected_total}",
                chunk.index, chunk.line
            )));
        }
        let slot = &mut slices[chunk.index - 1];
        if slot.is_some() {
            return Err(lint_parse_error(format!(
                "duplicate lint measurement chunk index {} at line {}",
                chunk.index, chunk.line
            )));
        }
        *slot = Some(chunk.slice);
    }

    let mut payload = String::new();
    for (index, slice) in slices.into_iter().enumerate() {
        let Some(slice) = slice else {
            return Err(lint_parse_error(format!(
                "missing lint measurement chunk index {}",
                index + 1
            )));
        };
        payload.push_str(slice);
    }
    Ok(payload)
}

fn first_missing_chunk_index(chunks: &[LintPayloadChunk<'_>]) -> usize {
    (1..=chunks.len() + 1)
        .find(|index| !chunks.iter().any(|chunk| chunk.index == *index))
        .unwrap_or(chunks.len() + 1)
}

fn lint_payload_chunks(chrome_log: &str) -> miette::Result<Vec<LintPayloadChunk<'_>>> {
    let mut chunks = Vec::new();
    let mut search_start = 0;
    while let Some(relative_start) = chrome_log[search_start..].find(PEITHO_LINT_CHUNK) {
        let start = search_start + relative_start;
        chunks.push(parse_lint_payload_chunk(chrome_log, start)?);
        search_start = start + PEITHO_LINT_CHUNK.len();
    }
    Ok(chunks)
}

fn parse_lint_payload_chunk(
    chrome_log: &str,
    start: usize,
) -> miette::Result<LintPayloadChunk<'_>> {
    let line = line_number_at(chrome_log, start);
    let bytes = chrome_log.as_bytes();
    let mut cursor = start + PEITHO_LINT_CHUNK.len();
    consume_ascii_whitespace(bytes, &mut cursor);
    let index = parse_usize_field(chrome_log, bytes, &mut cursor, line, "chunk index")?;
    if bytes.get(cursor) != Some(&b'/') {
        return Err(lint_parse_error(format!(
            "malformed lint measurement chunk at line {line}: missing '/' after chunk index"
        )));
    }
    cursor += 1;
    let total = parse_usize_field(chrome_log, bytes, &mut cursor, line, "chunk total")?;
    if !matches!(bytes.get(cursor), Some(byte) if byte.is_ascii_whitespace()) {
        return Err(lint_parse_error(format!(
            "malformed lint measurement chunk at line {line}: missing space before chunk payload"
        )));
    }
    consume_ascii_whitespace(bytes, &mut cursor);
    let slice_start = cursor;
    while matches!(bytes.get(cursor), Some(byte) if is_base64_byte(*byte)) {
        cursor += 1;
    }
    if cursor == slice_start {
        return Err(lint_parse_error(format!(
            "malformed lint measurement chunk at line {line}: missing base64 payload"
        )));
    }
    Ok(LintPayloadChunk {
        index,
        total,
        slice: &chrome_log[slice_start..cursor],
        line,
    })
}

fn parse_usize_field(
    chrome_log: &str,
    bytes: &[u8],
    cursor: &mut usize,
    line: usize,
    field: &str,
) -> miette::Result<usize> {
    let start = *cursor;
    while matches!(bytes.get(*cursor), Some(byte) if byte.is_ascii_digit()) {
        *cursor += 1;
    }
    if *cursor == start {
        return Err(lint_parse_error(format!(
            "malformed lint measurement chunk at line {line}: missing {field}"
        )));
    }
    chrome_log[start..*cursor].parse::<usize>().map_err(|err| {
        lint_parse_error(format!(
            "malformed lint measurement chunk at line {line}: invalid {field}\ncaused by: {err}"
        ))
    })
}

fn consume_ascii_whitespace(bytes: &[u8], cursor: &mut usize) {
    while matches!(bytes.get(*cursor), Some(byte) if byte.is_ascii_whitespace()) {
        *cursor += 1;
    }
}

fn is_base64_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

fn line_number_at(input: &str, byte_index: usize) -> usize {
    input[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn lint_parse_error(message: String) -> miette::Report {
    miette::miette!(help = LINT_PARSE_HELP, "{message}")
}

fn collect_overflow_warnings(measurements: &[SlideMeasurement]) -> Vec<OverflowWarning> {
    let mut warnings = Vec::new();
    for measurement in measurements {
        let content_width = round_px(measurement.content_width);
        let content_height = round_px(measurement.content_height);
        let box_width = round_px(measurement.box_width);
        let box_height = round_px(measurement.box_height);
        let overflow_x = content_width - box_width;
        let overflow_y = content_height - box_height;
        if overflow_x > OVERFLOW_TOLERANCE_PX {
            warnings.push(OverflowWarning {
                slide: measurement.slide,
                axis: OverflowAxis::Horizontal,
                overflow_px: overflow_x,
                content_px: content_width,
                box_px: box_width,
            });
        }
        if overflow_y > OVERFLOW_TOLERANCE_PX {
            warnings.push(OverflowWarning {
                slide: measurement.slide,
                axis: OverflowAxis::Vertical,
                overflow_px: overflow_y,
                content_px: content_height,
                box_px: box_height,
            });
        }
    }
    warnings
}

fn collect_slot_overflow_warnings(measurements: &[SlideMeasurement]) -> Vec<SlotOverflowWarning> {
    let mut warnings = Vec::new();
    for measurement in measurements {
        for overflow in &measurement.slot_overflows {
            let (Some(axis), Some(overflow_px)) = (overflow.axis, overflow.overflow_px) else {
                continue;
            };
            if overflow_px > OVERFLOW_TOLERANCE_PX {
                warnings.push(SlotOverflowWarning {
                    slide: measurement.slide,
                    axis,
                    overflow_px,
                    overflow_value: overflow.overflow_value,
                    slot: overflow.slot.clone(),
                });
            }
        }
    }
    warnings
}

fn round_px(value: f64) -> i64 {
    value.round() as i64
}

fn round_font_size_pt_for_display(font_size_px: f64) -> f64 {
    (font_size_px * 0.75 * 10.0).round() / 10.0
}

fn collect_font_size_warnings(measurements: &[SlideMeasurement]) -> Vec<FontSizeWarning> {
    measurements
        .iter()
        .filter_map(|measurement| {
            let font_size_pt = round_font_size_pt_for_display(measurement.min_font_size_px?);
            (font_size_pt < MIN_FONT_SIZE_PT).then(|| FontSizeWarning {
                slide: measurement.slide,
                font_size_pt,
                sample: measurement.min_font_sample.clone().unwrap_or_default(),
            })
        })
        .collect()
}

fn format_rounded_font_size_pt(font_size_pt: f64) -> String {
    if font_size_pt.fract() == 0.0 {
        format!("{font_size_pt:.0}pt")
    } else {
        format!("{font_size_pt:.1}pt")
    }
}

fn write_lint_report(
    measurements: &[SlideMeasurement],
    stdout: &mut dyn Write,
) -> miette::Result<i32> {
    let overflow_warnings = collect_overflow_warnings(measurements);
    let slot_overflow_warnings = collect_slot_overflow_warnings(measurements);
    let font_size_warnings = collect_font_size_warnings(measurements);
    for warning in &overflow_warnings {
        writeln!(
            stdout,
            "warning: slide {} content overflows the slide box {} by {}px (content {}px, box {}px)",
            warning.slide,
            warning.axis.adverb(),
            warning.overflow_px,
            warning.content_px,
            warning.box_px
        )
        .into_diagnostic()?;
        writeln!(stdout, "  help: {OVERFLOW_HELP}").into_diagnostic()?;
    }
    for warning in &slot_overflow_warnings {
        let target = match &warning.slot {
            Some(slot) => format!("the `{slot}` slot"),
            None => "a container".to_owned(),
        };
        writeln!(
            stdout,
            "warning: slide {} content overflows {} {} by {}px",
            warning.slide,
            target,
            warning.axis.adverb(),
            warning.overflow_px
        )
        .into_diagnostic()?;
        let help = warning
            .overflow_value
            .map_or(OVERFLOW_HELP, OverflowValue::help);
        writeln!(stdout, "  help: {help}").into_diagnostic()?;
    }
    for warning in &font_size_warnings {
        writeln!(
            stdout,
            "warning: slide {} has text at {}, below the recommended 24pt: \"{}\"",
            warning.slide,
            format_rounded_font_size_pt(warning.font_size_pt),
            warning.sample
        )
        .into_diagnostic()?;
        writeln!(stdout, "  help: {FONT_SIZE_HELP}").into_diagnostic()?;
    }

    let warning_count =
        overflow_warnings.len() + slot_overflow_warnings.len() + font_size_warnings.len();
    if warning_count == 0 {
        writeln!(
            stdout,
            "checked {} slide(s): no warnings",
            measurements.len()
        )
        .into_diagnostic()?;
        Ok(0)
    } else {
        writeln!(
            stdout,
            "checked {} slide(s): {} warning(s)",
            measurements.len(),
            warning_count
        )
        .into_diagnostic()?;
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, path::Path};

    fn encoded(json: &str) -> String {
        STANDARD.encode(json.as_bytes())
    }

    fn console_line(message: &str) -> String {
        format!(
            r#"[123:456:0715/120000.000000:INFO:CONSOLE(59)] "{message}", source: file:///tmp/peitho/lint.html (59)"#
        )
    }

    fn console_chunk(index: usize, total: usize, slice: &str) -> String {
        console_line(&format!("PEITHO_LINT_CHUNK {index}/{total} {slice}"))
    }

    fn chunked_console_log(payload: &str, split_at: usize) -> String {
        let (first, second) = payload.split_at(split_at);
        format!(
            "GPU noise before chunks\n{}\n[GPU] SharedImageManager error that must not splice chunks\n{}\n[123:456:0715/120000.000000:INFO:CONSOLE(59)] \"PEITHO_LINT_DONE\", source: file:///tmp/peitho/lint.html (59)",
            console_chunk(2, 2, second),
            console_chunk(1, 2, first)
        )
    }

    fn measurement(slide: usize, px: Option<f64>, sample: Option<&str>) -> SlideMeasurement {
        SlideMeasurement {
            slide,
            content_width: 800.0,
            content_height: 600.0,
            box_width: 800.0,
            box_height: 600.0,
            min_font_size_px: px,
            min_font_sample: sample.map(str::to_owned),
            slot_overflows: Vec::new(),
        }
    }

    fn slot_overflow(
        axis: Option<OverflowAxis>,
        overflow_px: Option<i64>,
        slot: Option<&str>,
    ) -> SlotOverflowMeasurement {
        slot_overflow_with_value(axis, overflow_px, None, slot)
    }

    fn slot_overflow_with_value(
        axis: Option<OverflowAxis>,
        overflow_px: Option<i64>,
        overflow_value: Option<OverflowValue>,
        slot: Option<&str>,
    ) -> SlotOverflowMeasurement {
        SlotOverflowMeasurement {
            axis,
            overflow_px,
            overflow_value,
            slot: slot.map(str::to_owned),
        }
    }

    fn slot_measurement(
        slide: usize,
        slot_overflows: Vec<SlotOverflowMeasurement>,
    ) -> SlideMeasurement {
        SlideMeasurement {
            slot_overflows,
            ..measurement(slide, None, None)
        }
    }

    fn assert_parse_error_mentions(
        stderr: &str,
        expected_slide_count: usize,
        needle: &str,
    ) -> String {
        let err = parse_lint_measurements(stderr, expected_slide_count).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(needle),
            "expected {needle:?} in {message:?}"
        );
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("chrome-stderr.log"), "actual help: {help}");
        message
    }

    #[test]
    fn emit_lint_workspace_writes_theme_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let workspace = dir.path().join("lint-workspace");
        fs::write(&deck, "# Intro\n").unwrap();
        let artifacts = crate::build_artifacts(&deck).unwrap();

        emit_lint_workspace(&workspace, &artifacts).unwrap();

        assert!(workspace.join("lint.html").is_file());
        for font in peitho_core::theme_fonts() {
            assert_eq!(
                fs::read(workspace.join("theme-fonts").join(font.file_name())).unwrap(),
                font.bytes()
            );
        }
    }

    #[test]
    fn lint_measurement_chunks_reassemble_base64_json_and_validate_slide_count() {
        let payload = encoded(
            r#"[{"slide":1,"contentWidth":1280.4,"contentHeight":762.49,"boxWidth":1280.0,"boxHeight":720.0,"minFontSizePx":18.0,"minFontSample":"Tiny text"}]"#,
        );
        let stderr = chunked_console_log(&payload, 24);

        let measurements = parse_lint_measurements(&stderr, 1).unwrap();
        assert_eq!(measurements[0].min_font_size_px, Some(18.0));
        assert_eq!(
            measurements[0].min_font_sample.as_deref(),
            Some("Tiny text")
        );

        assert_eq!(
            measurements,
            vec![SlideMeasurement {
                slide: 1,
                content_width: 1280.4,
                content_height: 762.49,
                box_width: 1280.0,
                box_height: 720.0,
                min_font_size_px: Some(18.0),
                min_font_sample: Some("Tiny text".to_owned()),
                slot_overflows: Vec::new(),
            }]
        );
    }

    #[test]
    fn lint_measurement_payload_defaults_missing_optional_fields_to_none() {
        let payload = encoded(
            r#"[{"slide":1,"contentWidth":1280.0,"contentHeight":720.0,"boxWidth":1280.0,"boxHeight":720.0}]"#,
        );

        let measurements = parse_lint_measurements(&console_chunk(1, 1, &payload), 1).unwrap();

        assert_eq!(measurements[0].min_font_size_px, None);
        assert_eq!(measurements[0].min_font_sample, None);
        assert!(measurements[0].slot_overflows.is_empty());
    }

    #[test]
    fn lint_measurement_payload_deserializes_slot_overflow_fields() {
        let payload = encoded(
            r#"[{"slide":1,"contentWidth":1280.0,"contentHeight":720.0,"boxWidth":1280.0,"boxHeight":720.0,"slotOverflows":[{"slotOverflowAxis":"horizontal","slotOverflowPx":7,"slotOverflowValue":"scroll","slotName":"body"},{"slotOverflowAxis":"vertical","slotOverflowPx":14,"slotName":"code"}]}]"#,
        );

        let measurements = parse_lint_measurements(&console_chunk(1, 1, &payload), 1).unwrap();

        assert_eq!(
            measurements[0].slot_overflows,
            vec![
                slot_overflow_with_value(
                    Some(OverflowAxis::Horizontal),
                    Some(7),
                    Some(OverflowValue::Scroll),
                    Some("body"),
                ),
                slot_overflow(Some(OverflowAxis::Vertical), Some(14), Some("code"),),
            ]
        );
    }

    #[test]
    fn lint_measurement_payload_accepts_utf8_min_font_sample() {
        let payload = encoded(
            r#"[{"slide":1,"contentWidth":1280.0,"contentHeight":720.0,"boxWidth":1280.0,"boxHeight":720.0,"minFontSizePx":24.0,"minFontSample":"日本語の小さい文字🙂"}]"#,
        );

        let measurements = parse_lint_measurements(&console_chunk(1, 1, &payload), 1).unwrap();

        assert_eq!(
            measurements[0].min_font_sample.as_deref(),
            Some("日本語の小さい文字🙂")
        );
    }

    #[test]
    fn lint_measurement_chunk_errors_are_distinct_and_actionable() {
        let missing = assert_parse_error_mentions(
            "Chrome stderr without lint chunks",
            1,
            "no lint measurement chunks",
        );
        let inconsistent = assert_parse_error_mentions(
            &format!(
                "{}\n{}",
                console_chunk(1, 2, "YWJj"),
                console_chunk(2, 3, "ZA==")
            ),
            1,
            "inconsistent lint measurement chunk totals",
        );
        let duplicate = assert_parse_error_mentions(
            &format!(
                "{}\n{}",
                console_chunk(1, 2, "YWJj"),
                console_chunk(1, 2, "ZA==")
            ),
            1,
            "duplicate lint measurement chunk index",
        );
        let missing_index = assert_parse_error_mentions(
            &console_chunk(1, 2, "YWJj"),
            1,
            "missing lint measurement chunk index",
        );
        let _absurd_total = assert_parse_error_mentions(
            &console_chunk(1, usize::MAX, "YWJj"),
            1,
            "missing lint measurement chunk index",
        );
        let missing_header_index = assert_parse_error_mentions(
            &console_line("PEITHO_LINT_CHUNK /1 YWJj"),
            1,
            "missing chunk index",
        );
        let missing_slash = assert_parse_error_mentions(
            &console_line("PEITHO_LINT_CHUNK 1 1 YWJj"),
            1,
            "missing '/' after chunk index",
        );
        let missing_header_total = assert_parse_error_mentions(
            &console_line("PEITHO_LINT_CHUNK 1/ YWJj"),
            1,
            "missing chunk total",
        );
        let missing_payload_space = assert_parse_error_mentions(
            &console_line("PEITHO_LINT_CHUNK 1/1YWJj"),
            1,
            "missing space before chunk payload",
        );
        let missing_payload = assert_parse_error_mentions(
            &console_line("PEITHO_LINT_CHUNK 1/1 "),
            1,
            "missing base64 payload",
        );
        let zero_total = assert_parse_error_mentions(
            &console_chunk(1, 0, "YWJj"),
            1,
            "total must be greater than zero",
        );
        let bad_base64 = assert_parse_error_mentions(&console_chunk(1, 1, "abc"), 1, "base64");
        let bad_json =
            assert_parse_error_mentions(&console_chunk(1, 1, &encoded("{bad json")), 1, "JSON");
        let mismatch = assert_parse_error_mentions(
            &console_chunk(
                1,
                1,
                &encoded(
                    r#"[{"slide":1,"contentWidth":1280,"contentHeight":762,"boxWidth":1280,"boxHeight":720,"minFontSizePx":null,"minFontSample":null}]"#,
                ),
            ),
            2,
            "slide count mismatch",
        );

        assert_ne!(missing, bad_base64);
        assert_ne!(missing, inconsistent);
        assert_ne!(inconsistent, duplicate);
        assert_ne!(duplicate, missing_index);
        let malformed_messages = [
            missing_header_index,
            missing_slash,
            missing_header_total,
            missing_payload_space,
            missing_payload,
            zero_total,
        ];
        for (left_index, left) in malformed_messages.iter().enumerate() {
            for right in malformed_messages.iter().skip(left_index + 1) {
                assert_ne!(left, right);
            }
        }
        assert_ne!(bad_base64, bad_json);
        assert_ne!(bad_json, mismatch);
    }

    #[test]
    fn font_size_warning_collection_decides_on_displayed_pt() {
        let measurements = [
            measurement(1, Some(31.93), Some("Tiny caption")),
            measurement(2, Some(31.94), Some("Rounds to threshold")),
            measurement(3, None, None),
        ];

        let warnings = collect_font_size_warnings(&measurements);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].slide, 1);
        assert!((warnings[0].font_size_pt - 23.9).abs() < f64::EPSILON);
        assert_eq!(warnings[0].sample, "Tiny caption");

        let mut stdout = Vec::new();
        assert_eq!(write_lint_report(&measurements, &mut stdout).unwrap(), 1);
        assert!(String::from_utf8(stdout).unwrap().contains(
            "warning: slide 1 has text at 23.9pt, below the recommended 24pt: \"Tiny caption\""
        ));
    }

    #[test]
    fn overflow_warning_collection_applies_strict_one_pixel_tolerance_per_axis() {
        let measurements = vec![
            SlideMeasurement {
                slide: 1,
                content_width: 1281.49,
                content_height: 720.0,
                box_width: 1280.0,
                box_height: 720.0,
                min_font_size_px: None,
                min_font_sample: None,
                slot_overflows: Vec::new(),
            },
            SlideMeasurement {
                slide: 2,
                content_width: 1281.51,
                content_height: 720.0,
                box_width: 1280.0,
                box_height: 720.0,
                min_font_size_px: None,
                min_font_sample: None,
                slot_overflows: Vec::new(),
            },
            SlideMeasurement {
                slide: 3,
                content_width: 1280.0,
                content_height: 715.0,
                box_width: 1280.0,
                box_height: 720.0,
                min_font_size_px: None,
                min_font_sample: None,
                slot_overflows: Vec::new(),
            },
            SlideMeasurement {
                slide: 4,
                content_width: 503.4,
                content_height: 604.4,
                box_width: 500.1,
                box_height: 600.1,
                min_font_size_px: None,
                min_font_sample: None,
                slot_overflows: Vec::new(),
            },
            SlideMeasurement {
                slide: 5,
                content_width: 100.4,
                content_height: 720.0,
                box_width: 98.6,
                box_height: 720.0,
                min_font_size_px: None,
                min_font_sample: None,
                slot_overflows: Vec::new(),
            },
        ];

        let warnings = collect_overflow_warnings(&measurements);

        assert_eq!(
            warnings,
            vec![
                OverflowWarning {
                    slide: 2,
                    axis: OverflowAxis::Horizontal,
                    overflow_px: 2,
                    content_px: 1282,
                    box_px: 1280,
                },
                OverflowWarning {
                    slide: 4,
                    axis: OverflowAxis::Horizontal,
                    overflow_px: 3,
                    content_px: 503,
                    box_px: 500,
                },
                OverflowWarning {
                    slide: 4,
                    axis: OverflowAxis::Vertical,
                    overflow_px: 4,
                    content_px: 604,
                    box_px: 600,
                },
            ]
        );
    }

    #[test]
    fn slot_overflow_warning_collection_handles_axes_slots_and_tolerance() {
        let measurements = [
            slot_measurement(1, Vec::new()),
            slot_measurement(
                2,
                vec![
                    slot_overflow_with_value(
                        Some(OverflowAxis::Horizontal),
                        Some(7),
                        Some(OverflowValue::Auto),
                        Some("code"),
                    ),
                    slot_overflow_with_value(
                        Some(OverflowAxis::Vertical),
                        Some(14),
                        Some(OverflowValue::Hidden),
                        None,
                    ),
                ],
            ),
            slot_measurement(
                3,
                vec![
                    slot_overflow(Some(OverflowAxis::Horizontal), Some(1), Some("body")),
                    slot_overflow(Some(OverflowAxis::Vertical), Some(2), Some("body")),
                ],
            ),
            slot_measurement(
                4,
                vec![
                    slot_overflow(Some(OverflowAxis::Horizontal), None, Some("code")),
                    slot_overflow(None, Some(9), Some("code")),
                ],
            ),
        ];

        let warnings = collect_slot_overflow_warnings(&measurements);

        assert_eq!(
            warnings,
            vec![
                SlotOverflowWarning {
                    slide: 2,
                    axis: OverflowAxis::Horizontal,
                    overflow_px: 7,
                    overflow_value: Some(OverflowValue::Auto),
                    slot: Some("code".to_owned()),
                },
                SlotOverflowWarning {
                    slide: 2,
                    axis: OverflowAxis::Vertical,
                    overflow_px: 14,
                    overflow_value: Some(OverflowValue::Hidden),
                    slot: None,
                },
                SlotOverflowWarning {
                    slide: 3,
                    axis: OverflowAxis::Vertical,
                    overflow_px: 2,
                    overflow_value: None,
                    slot: Some("body".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn lint_report_renders_named_and_unnamed_slot_overflow_warnings() {
        let measurements = [
            slot_measurement(
                10,
                vec![slot_overflow_with_value(
                    Some(OverflowAxis::Horizontal),
                    Some(8),
                    Some(OverflowValue::Hidden),
                    Some("code"),
                )],
            ),
            slot_measurement(
                11,
                vec![slot_overflow_with_value(
                    Some(OverflowAxis::Vertical),
                    Some(14),
                    Some(OverflowValue::Clip),
                    None,
                )],
            ),
        ];
        let mut stdout = Vec::new();

        let exit_code = write_lint_report(&measurements, &mut stdout).unwrap();

        assert_eq!(exit_code, 1);
        let output = String::from_utf8(stdout).unwrap();
        assert!(output
            .contains("warning: slide 10 content overflows the `code` slot horizontally by 8px"));
        assert!(
            output.contains("warning: slide 11 content overflows a container vertically by 14px")
        );
        assert_eq!(
            output.matches(&format!("  help: {OVERFLOW_HELP}")).count(),
            2
        );
        assert!(output.contains("checked 2 slide(s): 2 warning(s)"));
    }

    #[test]
    fn lint_report_uses_scrollable_help_for_auto_and_scroll_overflow() {
        let measurements = [slot_measurement(
            12,
            vec![
                slot_overflow_with_value(
                    Some(OverflowAxis::Horizontal),
                    Some(8),
                    Some(OverflowValue::Auto),
                    Some("code"),
                ),
                slot_overflow_with_value(
                    Some(OverflowAxis::Vertical),
                    Some(14),
                    Some(OverflowValue::Scroll),
                    Some("code"),
                ),
            ],
        )];
        let mut stdout = Vec::new();

        let exit_code = write_lint_report(&measurements, &mut stdout).unwrap();

        assert_eq!(exit_code, 1);
        let output = String::from_utf8(stdout).unwrap();
        assert_eq!(
            output
                .matches(&format!("  help: {SCROLLABLE_OVERFLOW_HELP}"))
                .count(),
            2
        );
        assert!(!output.contains(&format!("  help: {OVERFLOW_HELP}")));
        assert!(output.contains("checked 1 slide(s): 2 warning(s)"));
    }

    #[test]
    fn lint_report_renders_warnings_summary_and_exit_code() {
        let measurements = vec![SlideMeasurement {
            slide: 3,
            content_width: 900.0,
            content_height: 642.4,
            box_width: 900.0,
            box_height: 600.2,
            min_font_size_px: Some(24.0),
            min_font_sample: Some("excerpt…".to_owned()),
            slot_overflows: Vec::new(),
        }];
        let mut stdout = Vec::new();

        let exit_code = write_lint_report(&measurements, &mut stdout).unwrap();

        assert_eq!(exit_code, 1);
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains(
            "warning: slide 3 content overflows the slide box vertically by 42px (content 642px, box 600px)"
        ));
        assert!(
            output.contains("  help: shrink or split the slide content, or adjust the layout CSS")
        );
        assert!(output.contains(
            "warning: slide 3 has text at 18pt, below the recommended 24pt: \"excerpt…\""
        ));
        assert!(output.contains(
            "  help: raise the font size in the layout CSS, or move content to another slide instead of shrinking it"
        ));
        assert!(output.contains("checked 1 slide(s): 2 warning(s)"));
        assert_eq!(format_rounded_font_size_pt(17.3), "17.3pt");
        assert_eq!(format_rounded_font_size_pt(24.0), "24pt");

        let mut clean_stdout = Vec::new();
        let clean = SlideMeasurement {
            slide: 1,
            content_width: 800.0,
            content_height: 600.0,
            box_width: 800.0,
            box_height: 600.0,
            min_font_size_px: None,
            min_font_sample: None,
            slot_overflows: Vec::new(),
        };
        assert_eq!(write_lint_report(&[clean], &mut clean_stdout).unwrap(), 0);
        assert_eq!(
            String::from_utf8(clean_stdout).unwrap(),
            "checked 1 slide(s): no warnings\n"
        );
    }

    #[test]
    fn chrome_stderr_log_write_failure_keeps_parse_error_primary() {
        let parse_error = lint_parse_error("primary parse failure".to_owned());
        let write_error = miette::miette!("disk refused chrome-stderr.log");

        let message = append_chrome_stderr_log_write_failure(parse_error, write_error).to_string();

        assert!(
            message.contains("primary parse failure"),
            "actual error: {message}"
        );
        assert!(
            message.contains("failed to write chrome-stderr.log"),
            "actual error: {message}"
        );
        assert!(
            message.contains("disk refused chrome-stderr.log"),
            "actual error: {message}"
        );
    }

    #[test]
    fn lint_uses_pdf_chrome_args_with_stderr_console_logging() {
        let profile = Path::new("/tmp/peitho-lint/chrome-profile");
        let pdf = Path::new("/tmp/peitho-lint/lint.pdf");
        let url = "file:///tmp/peitho-lint/lint.html";

        let args = crate::chrome_print_args(profile, pdf, url);

        assert_eq!(
            args,
            vec![
                OsString::from("--headless=new"),
                OsString::from("--disable-gpu"),
                OsString::from("--no-sandbox"),
                OsString::from("--no-pdf-header-footer"),
                OsString::from("--virtual-time-budget=10000"),
                OsString::from("--enable-logging=stderr"),
                OsString::from("--user-data-dir=/tmp/peitho-lint/chrome-profile"),
                OsString::from("--print-to-pdf=/tmp/peitho-lint/lint.pdf"),
                OsString::from("file:///tmp/peitho-lint/lint.html"),
            ]
        );
    }
}
