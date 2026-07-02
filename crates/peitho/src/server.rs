use std::{
    fs,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
};

use tiny_http::{Header, Method, Response, Server, StatusCode};

pub(crate) fn resolve_request_path(root: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split('?').next().unwrap_or(url);
    if path.contains("://") {
        return None;
    }
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(root.join("present.html"));
    }

    let mut out = root.to_path_buf();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

pub(crate) fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub struct PresentServer {
    root: PathBuf,
    server: Server,
}

impl PresentServer {
    pub fn bind(root: PathBuf, port: u16) -> miette::Result<Self> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|err| miette::miette!("failed to bind present server: {err}"))?;
        Ok(Self { root, server })
    }

    pub fn addr(&self) -> SocketAddr {
        self.server
            .server_addr()
            .to_ip()
            .expect("present server binds TCP")
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/present.html", self.addr().port())
    }

    pub fn serve_forever(self) -> miette::Result<()> {
        for request in self.server.incoming_requests() {
            self.respond(request);
        }
        Ok(())
    }

    pub fn handle_one(self) -> miette::Result<()> {
        if let Some(request) = self.server.incoming_requests().next() {
            self.respond(request);
        }
        Ok(())
    }

    fn respond(&self, request: tiny_http::Request) {
        if request.method() != &Method::Get {
            send_response(request, Response::empty(StatusCode(405)));
            return;
        }
        let Some(path) = resolve_request_path(&self.root, request.url()) else {
            send_response(
                request,
                Response::from_string("404\n").with_status_code(StatusCode(404)),
            );
            return;
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let Ok(header) = Header::from_bytes("Content-Type", content_type(&path)) else {
                    eprintln!("warning: failed to build Content-Type header");
                    return;
                };
                send_response(request, Response::from_data(bytes).with_header(header));
            }
            Err(_) => {
                send_response(
                    request,
                    Response::from_string("404\n").with_status_code(StatusCode(404)),
                );
            }
        }
    }
}

fn send_response<R>(request: tiny_http::Request, response: Response<R>)
where
    R: std::io::Read + Send + 'static,
{
    if let Err(err) = request.respond(response) {
        eprintln!("warning: failed to send present server response: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolves_root_to_present_html() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/").unwrap(),
            Path::new("/cache").join("present.html")
        );
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(resolve_request_path(Path::new("/cache"), "/../manifest.json").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "/slides/../../secret").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "http://x/manifest.json").is_none());
    }

    #[test]
    fn maps_content_types() {
        assert_eq!(
            content_type(Path::new("present.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("peitho.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("shell.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("manifest.json")),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("slide.bin")),
            "application/octet-stream"
        );
    }
}
