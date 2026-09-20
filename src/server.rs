use crate::{bookmark, html};
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_REQUEST_BYTES: usize = 16 * 1024;

pub struct BookmarkServer {
    listener: TcpListener,
    app: App,
}

struct App {
    root: PathBuf,
    active_date: String,
}

impl BookmarkServer {
    pub fn bind(root: &Path, active_date: &str) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .context("failed to start the local bookmark server")?;
        Ok(Self {
            listener,
            app: App {
                root: root.to_owned(),
                active_date: active_date.to_owned(),
            },
        })
    }

    pub fn url(&self) -> Result<String> {
        let address = self.listener.local_addr()?;
        Ok(format!("http://{address}/{}/", self.app.active_date))
    }

    pub fn run(self) -> Result<()> {
        for connection in self.listener.incoming() {
            match connection {
                Ok(mut stream) => {
                    if let Err(error) = self.handle_connection(&mut stream) {
                        eprintln!("warning: local server request failed: {error:#}");
                    }
                }
                Err(error) => eprintln!("warning: local server connection failed: {error}"),
            }
        }
        Ok(())
    }

    fn handle_connection(&self, stream: &mut TcpStream) -> Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let request = read_request(stream)?;
        let response = self.app.route(&request);
        write_response(stream, response)
    }
}

impl App {
    fn route(&self, request: &Request) -> Response {
        if request.path == "/" && request.method == "GET" {
            return Response::redirect(format!("/{}/", self.active_date));
        }
        if request.path == "/bookmark.css" && request.method == "GET" {
            return Response::ok(
                "text/css; charset=utf-8",
                html::BOOKMARK_STYLE.as_bytes().to_vec(),
            );
        }
        if request.path == "/bookmark.js" && request.method == "GET" {
            return Response::ok(
                "text/javascript; charset=utf-8",
                html::BOOKMARK_SCRIPT.as_bytes().to_vec(),
            );
        }
        if matches!(request.path.as_str(), "/bookmarks" | "/bookmarks/") && request.method == "GET"
        {
            return match bookmark::load(&self.root) {
                Ok(store) => Response::ok(
                    "text/html; charset=utf-8",
                    html::render_bookmarks_page(&store.bookmarks, &self.active_date).into_bytes(),
                )
                .no_store(),
                Err(error) => Response::server_error(error),
            };
        }
        if request.path.starts_with("/api/bookmarks/") {
            return self.route_api(request);
        }
        if request.method != "GET" {
            return Response::method_not_allowed();
        }
        self.serve_static(&request.path)
    }

    fn route_api(&self, request: &Request) -> Response {
        if request.method != "GET"
            && request
                .headers
                .get("x-daily-atcoder")
                .is_none_or(|value| value != "1")
        {
            return Response::json(403, json!({ "error": "missing request token" }));
        }

        if let Some(problem_id) = request.path.strip_prefix("/api/bookmarks/problem/") {
            if request.method != "DELETE" {
                return Response::method_not_allowed();
            }
            if problem_id.is_empty()
                || !problem_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Response::json(400, json!({ "error": "invalid problem id" }));
            }
            return match bookmark::remove_by_problem_id(&self.root, problem_id) {
                Ok(bookmarked) => {
                    Response::json(200, json!({ "bookmarked": bookmarked })).no_store()
                }
                Err(error) => Response::server_error(error),
            };
        }

        let route = request
            .path
            .trim_start_matches('/')
            .split('/')
            .collect::<Vec<_>>();
        if route.len() != 4 || route[0] != "api" || route[1] != "bookmarks" {
            return Response::not_found();
        }
        let date = route[2];
        let slot = route[3];
        let result = match request.method.as_str() {
            "GET" => bookmark::is_bookmarked(&self.root, date, slot),
            "POST" => bookmark::add(&self.root, date, slot),
            "DELETE" => bookmark::remove(&self.root, date, slot),
            _ => return Response::method_not_allowed(),
        };
        match result {
            Ok(bookmarked) => Response::json(200, json!({ "bookmarked": bookmarked })).no_store(),
            Err(error) => Response::json(400, json!({ "error": error.to_string() })).no_store(),
        }
    }

    fn serve_static(&self, request_path: &str) -> Response {
        let Some(path) = static_file_path(&self.root, request_path) else {
            return Response::not_found();
        };
        let Ok(mut bytes) = fs::read(&path) else {
            return Response::not_found();
        };
        let content_type = content_type(&path);
        if content_type == "text/html; charset=utf-8" {
            let Ok(page) = String::from_utf8(bytes) else {
                return Response::json(500, json!({ "error": "HTML file is not UTF-8" }));
            };
            bytes = inject_bookmark_assets(&page).into_bytes();
        }
        Response::ok(content_type, bytes).no_store()
    }
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > MAX_REQUEST_BYTES {
            anyhow::bail!("HTTP request headers are too large");
        }
    }

    let text = std::str::from_utf8(&bytes).context("HTTP request is not UTF-8")?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().context("missing HTTP request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().context("missing HTTP method")?.to_owned();
    let target = parts.next().context("missing HTTP request target")?;
    let _version = parts.next().context("missing HTTP version")?;
    if parts.next().is_some() {
        anyhow::bail!("invalid HTTP request line");
    }
    let path = target.split('?').next().unwrap_or(target).to_owned();
    if !path.starts_with('/') {
        anyhow::bail!("invalid HTTP request target");
    }

    let mut headers = HashMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    Ok(Request {
        method,
        path,
        headers,
    })
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

impl Response {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type,
            body,
            headers: Vec::new(),
        }
    }

    fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec(&value).expect("JSON value is serializable"),
            headers: Vec::new(),
        }
    }

    fn redirect(location: String) -> Self {
        Self {
            status: 302,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
            headers: vec![("Location".into(), location)],
        }
    }

    fn not_found() -> Self {
        Self::json(404, json!({ "error": "not found" }))
    }

    fn method_not_allowed() -> Self {
        Self::json(405, json!({ "error": "method not allowed" }))
    }

    fn server_error(error: impl std::fmt::Display) -> Self {
        eprintln!("warning: local server error: {error}");
        Self::json(500, json!({ "error": "internal server error" }))
    }

    fn no_store(mut self) -> Self {
        self.headers
            .push(("Cache-Control".into(), "no-store".into()));
        self
    }
}

fn write_response(stream: &mut TcpStream, response: Response) -> Result<()> {
    let reason = match response.status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    )?;
    for (name, value) in response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn static_file_path(root: &Path, request_path: &str) -> Option<PathBuf> {
    let route = request_path.trim_start_matches('/');
    let (date, relative) = route.split_once('/').unwrap_or((route, ""));
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return None;
    }
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    if relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(root.join("contests").join(date).join(relative))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn inject_bookmark_assets(page: &str) -> String {
    let mut assets = String::new();
    if !page.contains("href=\"/bookmark.css\"") {
        assets.push_str("<link rel=\"stylesheet\" href=\"/bookmark.css\">\n");
    }
    if !page.contains("src=\"/bookmark.js\"") {
        assets.push_str("<script defer src=\"/bookmark.js\"></script>\n");
    }
    if assets.is_empty() {
        return page.to_owned();
    }
    page.replacen("</head>", &format!("{assets}</head>"), 1)
}

#[cfg(test)]
mod tests {
    use super::{inject_bookmark_assets, static_file_path, App, Request};
    use crate::model::{ContestFile, ContestProblemFile};
    use std::{
        collections::HashMap,
        fs,
        net::SocketAddr,
        path::{Path, PathBuf},
    };

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "daily-atcoder-server-{name}-{}",
            std::process::id()
        ))
    }

    fn create_contest(root: &Path) {
        let dir = root.join("contests/2026-09-20");
        fs::create_dir_all(&dir).unwrap();
        let contest = ContestFile {
            date: "2026-09-20".into(),
            problems: vec![ContestProblemFile {
                slot: "q1".into(),
                problem_id: "abc123_d".into(),
                contest_id: "abc123".into(),
                problem_index: "D".into(),
                title: "Example".into(),
                difficulty: 1200,
                raw_difficulty: 1200.0,
                url: "https://example.com/problem".into(),
                submit_url: "https://example.com/submit".into(),
            }],
        };
        fs::write(
            dir.join("contest.json"),
            serde_json::to_vec(&contest).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("q1.html"),
            "<html><head></head><body>old page</body></html>",
        )
        .unwrap();
    }

    #[test]
    fn existing_pages_receive_bookmark_assets_once() {
        let old = "<html><head><title>Q1</title></head><body></body></html>";
        let upgraded = inject_bookmark_assets(old);
        assert!(upgraded.contains("href=\"/bookmark.css\""));
        assert!(upgraded.contains("src=\"/bookmark.js\""));

        let unchanged = inject_bookmark_assets(&upgraded);
        assert_eq!(unchanged, upgraded);
    }

    #[test]
    fn static_paths_reject_traversal() {
        let root = Path::new("/tmp/example");
        assert_eq!(
            static_file_path(root, "/2026-09-20/q1.html").unwrap(),
            root.join("contests/2026-09-20/q1.html")
        );
        assert!(static_file_path(root, "/2026-09-20/../secret").is_none());
        assert!(static_file_path(root, "/not-a-date/q1.html").is_none());
    }

    #[test]
    fn api_adds_reads_and_removes_a_bookmark() {
        let root = test_root("api");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        create_contest(&root);
        let app = App {
            root: root.clone(),
            active_date: "2026-09-20".into(),
        };
        let mut headers = HashMap::new();
        headers.insert("x-daily-atcoder".into(), "1".into());

        let add = app.route(&Request {
            method: "POST".into(),
            path: "/api/bookmarks/2026-09-20/q1".into(),
            headers: headers.clone(),
        });
        assert_eq!(add.status, 200);
        assert!(String::from_utf8(add.body).unwrap().contains("true"));

        let read = app.route(&Request {
            method: "GET".into(),
            path: "/api/bookmarks/2026-09-20/q1".into(),
            headers: HashMap::new(),
        });
        assert_eq!(read.status, 200);
        assert!(String::from_utf8(read.body).unwrap().contains("true"));

        let remove = app.route(&Request {
            method: "DELETE".into(),
            path: "/api/bookmarks/2026-09-20/q1".into(),
            headers,
        });
        assert_eq!(remove.status, 200);
        assert!(String::from_utf8(remove.body).unwrap().contains("false"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn server_upgrades_an_existing_generated_page() {
        let root = test_root("legacy-page");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        create_contest(&root);
        let app = App {
            root: root.clone(),
            active_date: "2026-09-20".into(),
        };

        let response = app.serve_static("/2026-09-20/q1.html");
        assert_eq!(response.status, 200);
        let page = String::from_utf8(response.body).unwrap();
        assert!(page.contains("/bookmark.js"));
        assert!(page.contains("old page"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn socket_address_formats_as_browser_url() {
        let address: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert_eq!(
            format!("http://{address}/2026-09-20/"),
            "http://127.0.0.1:12345/2026-09-20/"
        );
    }
}
