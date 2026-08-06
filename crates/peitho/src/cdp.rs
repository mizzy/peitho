use std::{
    fmt, fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use tungstenite::{protocol::WebSocketConfig, Message, WebSocket};

const DEVTOOLS_PORT_FILE: &str = "DevToolsActivePort";
const HTTP_RESPONSE_LIMIT: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
// Total expression: it must never throw. The poll can land in a freshly
// created document before `<html>` is inserted, where `documentElement` is
// null (measured on CI, issue #418) — that window is "not ready", not an
// error. The `?? null` keeps the result null-or-string, so the strict
// RemoteObject decoding below stays exhaustive.
const PDF_FLATTENED_EXPRESSION: &str =
    "document.documentElement?.getAttribute('data-peitho-pdf-flattened') ?? null";

pub(crate) fn wait_for_devtools_port(
    profile: &Path,
    deadline: Instant,
    mut ensure_chrome_running: impl FnMut() -> miette::Result<()>,
) -> miette::Result<u16> {
    let port_file = profile.join(DEVTOOLS_PORT_FILE);
    loop {
        match fs::read_to_string(&port_file) {
            Ok(contents) if !contents.trim().is_empty() => {
                return parse_devtools_active_port(&contents);
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(miette::miette!(
                    "failed to read Chrome {DEVTOOLS_PORT_FILE} at {}: {err}",
                    port_file.display()
                ));
            }
        }

        ensure_chrome_running()?;
        sleep_for_poll(deadline, "waiting for Chrome DevToolsActivePort")?;
    }
}

fn parse_devtools_active_port(contents: &str) -> miette::Result<u16> {
    let first_line = contents.lines().next().ok_or_else(|| {
        miette::miette!("Chrome DevToolsActivePort is empty; expected a port on its first line")
    })?;
    let port = first_line.trim().parse::<u16>().map_err(|err| {
        miette::miette!("invalid Chrome DevToolsActivePort first line {first_line:?}: {err}")
    })?;
    if port == 0 {
        return Err(miette::miette!(
            "invalid Chrome DevToolsActivePort first line: port must be non-zero"
        ));
    }
    Ok(port)
}

pub(crate) fn fetch_page_websocket_url(port: u16, deadline: Instant) -> miette::Result<String> {
    wait_for_page_websocket_url_with(deadline, |attempt_deadline| {
        fetch_page_websocket_url_once(port, attempt_deadline)
    })
}

fn wait_for_page_websocket_url_with(
    deadline: Instant,
    mut fetch: impl FnMut(Instant) -> miette::Result<Option<String>>,
) -> miette::Result<String> {
    loop {
        if let Some(url) = fetch(deadline)? {
            return Ok(url);
        }
        sleep_for_poll(
            deadline,
            "waiting for Chrome page target with a WebSocket URL",
        )?;
    }
}

fn fetch_page_websocket_url_once(port: u16, deadline: Instant) -> miette::Result<Option<String>> {
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let connect_timeout = remaining(deadline, "connecting to Chrome's target list")?;
    let mut stream = TcpStream::connect_timeout(&address, connect_timeout).map_err(|err| {
        miette::miette!("failed to connect to Chrome target list at {address}: {err}")
    })?;
    configure_stream_deadline(&stream, deadline, "requesting Chrome's target list")?;

    let request =
        format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|err| {
        miette::miette!("failed to request Chrome target list from {address}: {err}")
    })?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        if let Some(body) = parse_http_body(&response)? {
            return parse_page_websocket_url(body);
        }
        if response.len() >= HTTP_RESPONSE_LIMIT {
            return Err(miette::miette!(
                "Chrome target list response exceeded {HTTP_RESPONSE_LIMIT} bytes"
            ));
        }

        configure_stream_deadline(&stream, deadline, "reading Chrome's target list")?;
        let read = stream.read(&mut buffer).map_err(|err| {
            if is_timeout(&err) {
                miette::miette!("timed out reading Chrome's target list")
            } else {
                miette::miette!("failed to read Chrome's target list: {err}")
            }
        })?;
        if read == 0 {
            return Err(miette::miette!(
                "Chrome closed its target-list connection before the Content-Length body was complete"
            ));
        }
        response.extend_from_slice(&buffer[..read]);
    }
}

fn parse_http_body(response: &[u8]) -> miette::Result<Option<&[u8]>> {
    let Some(header_end) = find_bytes(response, b"\r\n\r\n") else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|err| {
        miette::miette!("Chrome target list returned non-UTF-8 HTTP headers: {err}")
    })?;
    let mut lines = headers.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| miette::miette!("Chrome target list returned an empty HTTP response"))?;
    let status = status_line
        .split_ascii_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| {
            miette::miette!(
                "Chrome target list returned an invalid HTTP status line: {status_line}"
            )
        })?;
    if !(200..300).contains(&status) {
        return Err(miette::miette!(
            "Chrome target list returned HTTP status {status}"
        ));
    }

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then_some(value.trim())
        })
        .ok_or_else(|| {
            miette::miette!(
                "Chrome target list response is missing Content-Length; refusing to wait for EOF"
            )
        })?
        .parse::<usize>()
        .map_err(|err| {
            miette::miette!("Chrome target list returned an invalid Content-Length: {err}")
        })?;
    if content_length > HTTP_RESPONSE_LIMIT {
        return Err(miette::miette!(
            "Chrome target list Content-Length {content_length} exceeds {HTTP_RESPONSE_LIMIT} bytes"
        ));
    }

    let body_start = header_end + 4;
    let available = response.len().saturating_sub(body_start);
    if available < content_length {
        return Ok(None);
    }
    Ok(Some(&response[body_start..body_start + content_length]))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "type")]
    target_type: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_debugger_url: Option<String>,
}

fn parse_page_websocket_url(body: &[u8]) -> miette::Result<Option<String>> {
    let targets: Vec<Target> = serde_json::from_slice(body)
        .map_err(|err| miette::miette!("failed to parse Chrome target list JSON: {err}"))?;
    Ok(targets.into_iter().find_map(|target| {
        (target.target_type == "page")
            .then_some(target.websocket_debugger_url)
            .flatten()
    }))
}

fn loopback_websocket_url(port: u16, discovered_url: &str) -> miette::Result<String> {
    let remainder = discovered_url.strip_prefix("ws://").ok_or_else(|| {
        miette::miette!("Chrome page target returned a non-ws WebSocket URL: {discovered_url}")
    })?;
    let path_start = remainder.find('/').ok_or_else(|| {
        miette::miette!(
            "Chrome page target returned a WebSocket URL without a path: {discovered_url}"
        )
    })?;
    if path_start == 0 {
        return Err(miette::miette!(
            "Chrome page target returned a WebSocket URL without a host: {discovered_url}"
        ));
    }
    Ok(format!("ws://127.0.0.1:{port}{}", &remainder[path_start..]))
}

/// The sole WebSocket config source for `CdpClient::connect`, via
/// `connect_cdp_websocket`. The config test below fixes this unbounded-PDF
/// contract; callers must not perform a CDP handshake outside that helper.
fn cdp_websocket_config() -> WebSocketConfig {
    // Page.printToPDF returns the complete base64 PDF in one frame. The peer is
    // our trusted Chrome process on loopback, and PDF size has no inherent
    // bound, so tungstenite's defensive message and frame limits do not apply.
    WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None)
}

/// Performs the only WebSocket handshake used by `CdpClient::connect`, always
/// applying `cdp_websocket_config` rather than tungstenite's size-limited
/// defaults.
fn connect_cdp_websocket(url: &str, stream: TcpStream) -> miette::Result<WebSocket<TcpStream>> {
    let (socket, _) =
        tungstenite::client::client_with_config(url, stream, Some(cdp_websocket_config()))
            .map_err(|err| miette::miette!("failed to open Chrome CDP WebSocket: {err}"))?;
    Ok(socket)
}

pub(crate) struct CdpClient {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl CdpClient {
    pub(crate) fn connect(
        port: u16,
        discovered_url: &str,
        deadline: Instant,
    ) -> miette::Result<Self> {
        let url = loopback_websocket_url(port, discovered_url)?;
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        let connect_timeout = remaining(deadline, "connecting to Chrome's CDP WebSocket")?;
        let stream = TcpStream::connect_timeout(&address, connect_timeout).map_err(|err| {
            miette::miette!("failed to connect to Chrome CDP WebSocket at {address}: {err}")
        })?;
        stream
            .set_nodelay(true)
            .map_err(|err| miette::miette!("failed to configure Chrome CDP socket: {err}"))?;
        configure_stream_deadline(&stream, deadline, "opening Chrome's CDP WebSocket")?;
        let socket = connect_cdp_websocket(url.as_str(), stream)?;
        Ok(Self { socket, next_id: 1 })
    }

    pub(crate) fn page_enable(&mut self, deadline: Instant) -> miette::Result<()> {
        self.call("Page.enable", json!({}), deadline).map(|_| ())
    }

    pub(crate) fn page_navigate(&mut self, url: &str, deadline: Instant) -> miette::Result<()> {
        let result = self.call("Page.navigate", json!({ "url": url }), deadline)?;
        if let Some(error_text) = result.get("errorText").and_then(Value::as_str) {
            if !error_text.is_empty() {
                return Err(miette::miette!(
                    "Chrome failed to navigate to PDF workspace: {error_text}"
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn pdf_flattening_ready(&mut self, deadline: Instant) -> miette::Result<bool> {
        let result = self.call(
            "Runtime.evaluate",
            json!({
                "expression": PDF_FLATTENED_EXPRESSION,
                "returnByValue": true
            }),
            deadline,
        )?;
        if let Some(exception) = result.get("exceptionDetails") {
            return Err(miette::miette!(
                "Chrome failed to evaluate PDF flattening readiness: {exception}"
            ));
        }
        let remote_object = result.get("result").ok_or_else(|| {
            miette::miette!("Chrome Runtime.evaluate response omitted its result object")
        })?;
        match remote_object.get("value") {
            Some(Value::String(_)) => Ok(true),
            Some(Value::Null) => Ok(false),
            Some(value) => Err(miette::miette!(
                "Chrome returned unexpected PDF flattening readiness value: {value}"
            )),
            None if remote_object.get("subtype").and_then(Value::as_str) == Some("null") => {
                Ok(false)
            }
            None => Err(miette::miette!(
                "Chrome Runtime.evaluate response omitted the PDF flattening readiness value"
            )),
        }
    }

    pub(crate) fn page_print_to_pdf(&mut self, deadline: Instant) -> miette::Result<Vec<u8>> {
        let result = self.call(
            "Page.printToPDF",
            json!({
                "printBackground": true,
                "preferCSSPageSize": true,
                "displayHeaderFooter": false
            }),
            deadline,
        )?;
        let encoded = result.get("data").and_then(Value::as_str).ok_or_else(|| {
            miette::miette!("Chrome Page.printToPDF response omitted base64 PDF data")
        })?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| miette::miette!("Chrome returned invalid base64 PDF data: {err}"))
    }

    pub(crate) fn browser_close(&mut self, deadline: Instant) -> miette::Result<()> {
        self.call("Browser.close", json!({}), deadline).map(|_| ())
    }

    fn call(&mut self, method: &str, params: Value, deadline: Instant) -> miette::Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| miette::miette!("Chrome CDP command id overflow"))?;
        let request = json!({ "id": id, "method": method, "params": params }).to_string();

        configure_stream_deadline(
            self.socket.get_mut(),
            deadline,
            "sending a Chrome CDP command",
        )?;
        self.socket
            .send(Message::Text(request.into()))
            .map_err(|err| websocket_error(method, err))?;

        loop {
            configure_stream_deadline(
                self.socket.get_mut(),
                deadline,
                "waiting for a Chrome CDP response",
            )?;
            match self
                .socket
                .read()
                .map_err(|err| websocket_error(method, err))?
            {
                Message::Text(text) => {
                    if let Some(result) = correlate_cdp_response(text.as_str(), id)? {
                        return Ok(result);
                    }
                }
                Message::Close(frame) => {
                    return Err(miette::miette!(
                        "Chrome closed the CDP WebSocket while waiting for {method}: {frame:?}"
                    ));
                }
                Message::Binary(_) => {
                    return Err(miette::miette!(
                        "Chrome sent an unexpected binary CDP message while waiting for {method}"
                    ));
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }
}

pub(crate) fn wait_for_pdf_flattening(
    client: &mut CdpClient,
    deadline: Instant,
) -> miette::Result<()> {
    wait_for_pdf_flattening_with(deadline, |attempt_deadline| {
        client.pdf_flattening_ready(attempt_deadline)
    })
}

fn wait_for_pdf_flattening_with(
    deadline: Instant,
    mut readiness: impl FnMut(Instant) -> miette::Result<bool>,
) -> miette::Result<()> {
    loop {
        match readiness(deadline) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(err) if is_transient_navigation_context_error(&err) => {}
            Err(err) => return Err(err),
        }
        sleep_for_pdf_flattening_poll(deadline)?;
    }
}

fn sleep_for_pdf_flattening_poll(deadline: Instant) -> miette::Result<()> {
    sleep_for_poll(
        deadline,
        "waiting for PDF flattening readiness attribute data-peitho-pdf-flattened",
    )
    .map_err(|_| {
        miette::miette!(
            help = "PDF flattening readiness must appear in data-peitho-pdf-flattened before the export deadline",
            "timed out waiting for PDF flattening readiness attribute data-peitho-pdf-flattened"
        )
    })
}

#[derive(Debug)]
struct CdpProtocolError {
    code: Option<i64>,
    message: String,
}

impl fmt::Display for CdpProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(formatter, "Chrome CDP error {code}: {}", self.message),
            None => write!(formatter, "Chrome CDP error: {}", self.message),
        }
    }
}

impl std::error::Error for CdpProtocolError {}
impl miette::Diagnostic for CdpProtocolError {}

fn is_transient_navigation_context_error(err: &miette::Error) -> bool {
    let Some(protocol_error) = err.downcast_ref::<CdpProtocolError>() else {
        return false;
    };
    if protocol_error.code != Some(-32000) {
        return false;
    }

    // Chrome uses exactly these protocol messages while Page.navigate replaces
    // about:blank's default execution context with the PDF document's context.
    // Only that commit-window race is retryable; Runtime.exceptionDetails and
    // unrelated protocol failures remain fatal.
    [
        "Execution context was destroyed",
        "Cannot find default execution context",
    ]
    .iter()
    .any(|message| protocol_error.message.contains(message))
}

fn correlate_cdp_response(message: &str, expected_id: u64) -> miette::Result<Option<Value>> {
    let value: Value = serde_json::from_str(message)
        .map_err(|err| miette::miette!("Chrome sent invalid CDP JSON: {err}"))?;
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return Ok(None);
    };
    if id != expected_id {
        return Ok(None);
    }
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown CDP protocol error")
            .to_owned();
        let code = error.get("code").and_then(Value::as_i64);
        return Err(miette::Report::new(CdpProtocolError { code, message }));
    }
    value.get("result").cloned().map(Some).ok_or_else(|| {
        miette::miette!("Chrome CDP response {expected_id} omitted result and error")
    })
}

fn configure_stream_deadline(
    stream: &TcpStream,
    deadline: Instant,
    operation: &str,
) -> miette::Result<()> {
    let timeout = remaining(deadline, operation)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| miette::miette!("failed to set Chrome socket read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| miette::miette!("failed to set Chrome socket write timeout: {err}"))?;
    Ok(())
}

fn remaining(deadline: Instant, operation: &str) -> miette::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| miette::miette!("timed out {operation}"))
}

fn sleep_for_poll(deadline: Instant, operation: &str) -> miette::Result<()> {
    let remaining = remaining(deadline, operation)?;
    thread::sleep(POLL_INTERVAL.min(remaining));
    Ok(())
}

fn is_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

fn websocket_error(method: &str, err: tungstenite::Error) -> miette::Error {
    match &err {
        tungstenite::Error::Io(io_err) if is_timeout(io_err) => {
            miette::miette!("timed out waiting for Chrome CDP method {method}")
        }
        _ => miette::miette!("Chrome CDP method {method} failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devtools_active_port_uses_first_line() {
        assert_eq!(
            parse_devtools_active_port("43127\n/devtools/browser/abc\n").unwrap(),
            43127
        );
    }

    #[test]
    fn devtools_active_port_rejects_missing_invalid_and_zero_ports() {
        for contents in ["", "not-a-port\n", "0\n"] {
            let err = parse_devtools_active_port(contents).unwrap_err();
            assert!(
                err.to_string().contains("DevToolsActivePort"),
                "contents {contents:?}, actual error: {err}"
            );
        }
    }

    #[test]
    fn http_body_is_complete_at_content_length_without_waiting_for_eof() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\nhello worldbytes from a still-open connection";

        assert_eq!(
            parse_http_body(response).unwrap(),
            Some(&b"hello world"[..])
        );
    }

    #[test]
    fn http_body_waits_for_all_content_length_bytes() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello";

        assert_eq!(parse_http_body(response).unwrap(), None);
    }

    #[test]
    fn http_body_rejects_non_success_and_missing_content_length() {
        for response in [
            &b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\n\r\n[]"[..],
        ] {
            assert!(parse_http_body(response).is_err());
        }
    }

    #[test]
    fn target_list_selects_page_websocket_url() {
        let body = br#"[
            {"id":"worker","type":"service_worker","webSocketDebuggerUrl":"ws://localhost:43127/devtools/page/worker"},
            {"id":"page","type":"page","webSocketDebuggerUrl":"ws://localhost:43127/devtools/page/page-id"}
        ]"#;

        assert_eq!(
            parse_page_websocket_url(body).unwrap(),
            Some("ws://localhost:43127/devtools/page/page-id".to_owned())
        );
    }

    #[test]
    fn target_list_reports_not_ready_without_page_websocket_url() {
        for body in [
            &br#"[]"#[..],
            &br#"[{"type":"service_worker","webSocketDebuggerUrl":"ws://localhost/worker"}]"#[..],
            &br#"[{"type":"page"}]"#[..],
        ] {
            assert_eq!(parse_page_websocket_url(body).unwrap(), None);
        }

        let err = parse_page_websocket_url(b"not JSON").unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to parse Chrome target list JSON"),
            "actual error: {err}"
        );
    }

    #[test]
    fn page_target_discovery_retries_only_not_ready_results() {
        let mut attempts = 0;
        let url = wait_for_page_websocket_url_with(Instant::now() + Duration::from_secs(1), |_| {
            attempts += 1;
            if attempts < 3 {
                Ok(None)
            } else {
                Ok(Some("ws://127.0.0.1/devtools/page/ready".to_owned()))
            }
        })
        .unwrap();

        assert_eq!(url, "ws://127.0.0.1/devtools/page/ready");
        assert_eq!(attempts, 3);

        let mut fatal_attempts = 0;
        let err = wait_for_page_websocket_url_with(Instant::now() + Duration::from_secs(1), |_| {
            fatal_attempts += 1;
            Err(miette::miette!("invalid target-list JSON"))
        })
        .unwrap_err();
        assert!(err.to_string().contains("invalid target-list JSON"));
        assert_eq!(fatal_attempts, 1);
    }

    #[test]
    fn websocket_url_is_rewritten_to_ipv4_loopback() {
        assert_eq!(
            loopback_websocket_url(
                43127,
                "ws://localhost:43127/devtools/page/page-id?session=one"
            )
            .unwrap(),
            "ws://127.0.0.1:43127/devtools/page/page-id?session=one"
        );
    }

    #[test]
    fn cdp_connect_websocket_config_allows_unbounded_pdf_messages_and_frames() {
        let config = cdp_websocket_config();

        assert_eq!(config.max_message_size, None);
        assert_eq!(config.max_frame_size, None);
    }

    #[test]
    fn cdp_response_correlation_ignores_events_and_other_ids() {
        assert_eq!(
            correlate_cdp_response(r#"{"method":"Page.loadEventFired","params":{}}"#, 7).unwrap(),
            None
        );
        assert_eq!(
            correlate_cdp_response(r#"{"id":6,"result":{"ignored":true}}"#, 7).unwrap(),
            None
        );

        let result = correlate_cdp_response(r#"{"id":7,"result":{"frameId":"frame-1"}}"#, 7)
            .unwrap()
            .unwrap();
        assert_eq!(result["frameId"], "frame-1");
    }

    #[test]
    fn cdp_response_correlation_surfaces_protocol_errors() {
        let err = correlate_cdp_response(
            r#"{"id":7,"error":{"code":-32601,"message":"Method not found"}}"#,
            7,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("Method not found"),
            "actual error: {err}"
        );
    }

    #[test]
    fn navigation_context_protocol_errors_are_the_only_transient_readiness_errors() {
        for message in [
            "Execution context was destroyed.",
            "Cannot find default execution context",
        ] {
            let response = format!(
                r#"{{"id":7,"error":{{"code":-32000,"message":{}}}}}"#,
                serde_json::to_string(message).unwrap()
            );
            let err = correlate_cdp_response(&response, 7).unwrap_err();
            assert!(
                is_transient_navigation_context_error(&err),
                "actual error: {err}"
            );
        }

        let other_protocol_error = correlate_cdp_response(
            r#"{"id":7,"error":{"code":-32000,"message":"Permission denied"}}"#,
            7,
        )
        .unwrap_err();
        assert!(!is_transient_navigation_context_error(
            &other_protocol_error
        ));

        let exception_details = miette::miette!(
            "Chrome failed to evaluate PDF flattening readiness: Execution context was destroyed"
        );
        assert!(!is_transient_navigation_context_error(&exception_details));
    }

    #[test]
    fn flattening_readiness_retries_transient_navigation_context_errors() {
        let transient = correlate_cdp_response(
            r#"{"id":7,"error":{"code":-32000,"message":"Execution context was destroyed."}}"#,
            7,
        )
        .unwrap_err();
        let mut responses = std::collections::VecDeque::from([Err(transient), Ok(false), Ok(true)]);

        wait_for_pdf_flattening_with(Instant::now() + Duration::from_secs(1), |_| {
            responses.pop_front().unwrap()
        })
        .unwrap();

        assert!(responses.is_empty());
    }

    #[test]
    fn flattening_readiness_keeps_other_cdp_errors_fatal() {
        let fatal = correlate_cdp_response(
            r#"{"id":7,"error":{"code":-32000,"message":"Permission denied"}}"#,
            7,
        )
        .unwrap_err();
        let mut fatal = Some(fatal);
        let mut attempts = 0;

        let err = wait_for_pdf_flattening_with(Instant::now() + Duration::from_secs(1), |_| {
            attempts += 1;
            Err(fatal.take().unwrap())
        })
        .unwrap_err();

        assert!(err.to_string().contains("Permission denied"));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn flattening_readiness_timeout_has_readiness_specific_help() {
        let err = wait_for_pdf_flattening_with(Instant::now(), |_| Ok(false)).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("timed out waiting for PDF flattening readiness"),
            "actual error: {message}"
        );
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("PDF flattening readiness must appear in data-peitho-pdf-flattened"),
            "actual help: {help}"
        );
    }
}
