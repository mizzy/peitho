use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use miette::IntoDiagnostic;
use serde::Deserialize;

pub(crate) const PEITHO_LINT_DONE: &str = "PEITHO_LINT_DONE";
const PEITHO_LINT_CHUNK: &str = "PEITHO_LINT_CHUNK";
const OVERFLOW_TOLERANCE_PX: i64 = 1;
const OVERFLOW_HELP: &str = "shrink or split the slide content, or adjust the layout CSS";
const LINT_PARSE_HELP: &str =
    "rerun lint and inspect lint.html and chrome-stderr.log in the kept workspace";

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct SlideOverflow {
    slide: usize,
    #[serde(rename = "contentWidth")]
    content_width: f64,
    #[serde(rename = "contentHeight")]
    content_height: f64,
    #[serde(rename = "boxWidth")]
    box_width: f64,
    #[serde(rename = "boxHeight")]
    box_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverflowWarning {
    slide: usize,
    axis: OverflowAxis,
    overflow_px: i64,
    content_px: i64,
    box_px: i64,
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
            "failed to write Chrome stderr log to {}\nhelp: rerun lint and inspect lint.html in the kept workspace\ncaused by: {err}",
            log_path.display()
        )
    })
}

fn append_chrome_stderr_log_write_failure(
    parse_error: miette::Report,
    write_error: miette::Report,
) -> miette::Report {
    miette::miette!(
        "{}\nnote: failed to write chrome-stderr.log: {}",
        parse_error,
        write_error
    )
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
    let args = lint_chrome_args(&profile, &lint_pdf, &url);
    let output = crate::run_one_shot_chrome(
        chrome,
        &args,
        crate::ChromeCompletion::LintResultLogged,
        crate::CHROME_ONE_SHOT_TIMEOUT,
    )?;
    Ok(output.stderr)
}

fn lint_chrome_args(profile: &Path, pdf: &Path, url: &str) -> Vec<OsString> {
    let mut args = crate::chrome_print_args(profile, pdf, url);
    let insert_at = args
        .iter()
        .position(|arg| arg.to_string_lossy().starts_with("--user-data-dir="))
        .unwrap_or_else(|| args.len().saturating_sub(1));
    args.insert(insert_at, OsString::from("--enable-logging=stderr"));
    args
}

fn parse_lint_measurements(
    chrome_log: &str,
    expected_slide_count: usize,
) -> miette::Result<Vec<SlideOverflow>> {
    let payload = extract_lint_payload(chrome_log)?;
    let json = STANDARD.decode(payload).map_err(|err| {
        miette::miette!(
            "lint measurement payload is not valid base64\nhelp: {LINT_PARSE_HELP}\ncaused by: {err}"
        )
    })?;
    let measurements: Vec<SlideOverflow> = serde_json::from_slice(&json).map_err(|err| {
        miette::miette!(
            "lint measurement payload is not valid JSON\nhelp: {LINT_PARSE_HELP}\ncaused by: {err}"
        )
    })?;
    if measurements.len() != expected_slide_count {
        return Err(miette::miette!(
            "lint measurement slide count mismatch: expected {expected_slide_count}, got {}\nhelp: no lint result was accepted; {LINT_PARSE_HELP}",
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
    miette::miette!("{message}\nhelp: {LINT_PARSE_HELP}")
}

fn collect_overflow_warnings(measurements: &[SlideOverflow]) -> Vec<OverflowWarning> {
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

fn round_px(value: f64) -> i64 {
    value.round() as i64
}

fn write_lint_report(
    measurements: &[SlideOverflow],
    stdout: &mut dyn Write,
) -> miette::Result<i32> {
    let warnings = collect_overflow_warnings(measurements);
    for warning in &warnings {
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
    if warnings.is_empty() {
        writeln!(
            stdout,
            "checked {} slide(s): no overflow",
            measurements.len()
        )
        .into_diagnostic()?;
        Ok(0)
    } else {
        writeln!(
            stdout,
            "checked {} slide(s): {} overflow warning(s)",
            measurements.len(),
            warnings.len()
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
        assert!(message.contains("help:"), "actual error: {message}");
        assert!(
            message.contains("chrome-stderr.log"),
            "actual error: {message}"
        );
        message
    }

    #[test]
    fn lint_measurement_chunks_reassemble_base64_json_and_validate_slide_count() {
        let payload = encoded(
            r#"[{"slide":1,"contentWidth":1280.4,"contentHeight":762.49,"boxWidth":1280.0,"boxHeight":720.0}]"#,
        );
        let stderr = chunked_console_log(&payload, 24);

        let measurements = parse_lint_measurements(&stderr, 1).unwrap();

        assert_eq!(
            measurements,
            vec![SlideOverflow {
                slide: 1,
                content_width: 1280.4,
                content_height: 762.49,
                box_width: 1280.0,
                box_height: 720.0,
            }]
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
                    r#"[{"slide":1,"contentWidth":1280,"contentHeight":762,"boxWidth":1280,"boxHeight":720}]"#,
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
    fn overflow_warning_collection_applies_strict_one_pixel_tolerance_per_axis() {
        let measurements = vec![
            SlideOverflow {
                slide: 1,
                content_width: 1281.49,
                content_height: 720.0,
                box_width: 1280.0,
                box_height: 720.0,
            },
            SlideOverflow {
                slide: 2,
                content_width: 1281.51,
                content_height: 720.0,
                box_width: 1280.0,
                box_height: 720.0,
            },
            SlideOverflow {
                slide: 3,
                content_width: 1280.0,
                content_height: 715.0,
                box_width: 1280.0,
                box_height: 720.0,
            },
            SlideOverflow {
                slide: 4,
                content_width: 503.4,
                content_height: 604.4,
                box_width: 500.1,
                box_height: 600.1,
            },
            SlideOverflow {
                slide: 5,
                content_width: 100.4,
                content_height: 720.0,
                box_width: 98.6,
                box_height: 720.0,
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
    fn lint_report_renders_warnings_summary_and_exit_code() {
        let measurements = vec![SlideOverflow {
            slide: 3,
            content_width: 900.0,
            content_height: 642.4,
            box_width: 900.0,
            box_height: 600.2,
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
        assert!(output.contains("checked 1 slide(s): 1 overflow warning(s)"));

        let mut clean_stdout = Vec::new();
        let clean_exit = write_lint_report(
            &[SlideOverflow {
                slide: 1,
                content_width: 800.0,
                content_height: 601.4,
                box_width: 800.0,
                box_height: 600.0,
            }],
            &mut clean_stdout,
        )
        .unwrap();

        assert_eq!(clean_exit, 0);
        assert_eq!(
            String::from_utf8(clean_stdout).unwrap(),
            "checked 1 slide(s): no overflow\n"
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
    fn lint_chrome_args_print_pdf_and_enable_stderr_console_logging() {
        let profile = Path::new("/tmp/peitho-lint/chrome-profile");
        let pdf = Path::new("/tmp/peitho-lint/lint.pdf");
        let url = "file:///tmp/peitho-lint/lint.html";

        let args = lint_chrome_args(profile, pdf, url);

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
