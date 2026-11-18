use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use percent_encoding::percent_decode_str;
use serde::Serialize;
use uuid::Uuid;

use super::{commands::PluginCommandError, install::PluginAsset};

const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;

pub(super) type FrameAssetReader =
    Arc<dyn Fn(&str) -> Result<PluginAsset, PluginCommandError> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameSessionDescriptor {
    pub session_id: String,
    pub launch_token: String,
    pub url: String,
}

struct SessionControl {
    address: SocketAddr,
    accepting: Arc<AtomicBool>,
    available: Arc<RwLock<bool>>,
    thread: JoinHandle<()>,
}

#[derive(Clone, Default)]
pub struct FrameSessionRuntime {
    sessions: Arc<Mutex<HashMap<String, SessionControl>>>,
}

impl FrameSessionRuntime {
    pub(super) fn start(
        &self,
        entry: &str,
        reader: FrameAssetReader,
    ) -> Result<FrameSessionDescriptor, PluginCommandError> {
        let mut sessions = self.sessions.lock().map_err(|_| session_lock_error())?;
        validate_entry(entry, &reader)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(frame_server_error)?;
        listener.set_nonblocking(true).map_err(frame_server_error)?;
        let address = listener.local_addr().map_err(frame_server_error)?;
        let session_id = Uuid::new_v4().simple().to_string();
        let launch_token = Uuid::new_v4().simple().to_string();
        let origin = format!("http://{address}");
        let url = session_url(&origin, &session_id, &launch_token, entry)?;
        let accepting = Arc::new(AtomicBool::new(true));
        let available = Arc::new(RwLock::new(true));
        let server_accepting = accepting.clone();
        let server_available = available.clone();
        let server_session = Session {
            address,
            origin,
            entry: entry.to_string(),
            session_id: session_id.clone(),
            launch_token: launch_token.clone(),
            reader,
        };
        let thread = thread::Builder::new()
            .name(format!("plugin-frame-{session_id}"))
            .spawn(move || {
                serve(listener, server_session, server_accepting, server_available);
            })
            .map_err(frame_server_error)?;
        sessions.insert(
            session_id.clone(),
            SessionControl {
                address,
                accepting,
                available,
                thread,
            },
        );
        Ok(FrameSessionDescriptor {
            session_id,
            launch_token,
            url,
        })
    }

    pub fn revoke(&self, session_id: &str) -> bool {
        let control = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(session_id));
        let Some(control) = control else {
            return false;
        };
        control.accepting.store(false, Ordering::Release);
        if let Ok(mut available) = control.available.write() {
            *available = false;
        }
        let _ = TcpStream::connect_timeout(&control.address, Duration::from_millis(100));
        let _ = control.thread.join();
        true
    }
}

#[derive(Clone)]
struct Session {
    address: SocketAddr,
    origin: String,
    entry: String,
    session_id: String,
    launch_token: String,
    reader: FrameAssetReader,
}

fn validate_entry(entry: &str, reader: &FrameAssetReader) -> Result<(), PluginCommandError> {
    let asset = reader(entry)?;
    if asset.content_type != "text/html; charset=utf-8" {
        return Err(PluginCommandError::new(
            "PLUGIN_ENTRY_NOT_HTML",
            "Plugin UI entry must be an HTML document",
        ));
    }
    std::str::from_utf8(&asset.bytes).map_err(|_| {
        PluginCommandError::new(
            "PLUGIN_ENTRY_ENCODING_INVALID",
            "Plugin UI entry must be valid UTF-8",
        )
    })?;
    Ok(())
}

fn session_url(
    origin: &str,
    session_id: &str,
    launch_token: &str,
    entry: &str,
) -> Result<String, PluginCommandError> {
    let mut url = url::Url::parse(origin).map_err(frame_server_error)?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            PluginCommandError::new("PLUGIN_FRAME_URL_INVALID", "Plugin frame URL is invalid")
        })?;
        segments.push(session_id).push(launch_token);
        for segment in entry.split('/') {
            segments.push(segment);
        }
    }
    Ok(url.to_string())
}

fn serve(
    listener: TcpListener,
    session: Session,
    accepting: Arc<AtomicBool>,
    available: Arc<RwLock<bool>>,
) {
    while accepting.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if !accepting.load(Ordering::Acquire) {
                    break;
                }
                let session = session.clone();
                let available = available.clone();
                let _ = thread::Builder::new()
                    .name(format!("plugin-frame-request-{}", session.session_id))
                    .spawn(move || {
                        let _ = serve_request(stream, &session, &available);
                    });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn serve_request(
    mut stream: TcpStream,
    session: &Session,
    available: &RwLock<bool>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let availability = match available.read() {
        Ok(availability) if *availability => availability,
        _ => return write_error(&mut stream, 410, "Gone", "PLUGIN_FRAME_SESSION_REVOKED"),
    };
    let mut reader = BufReader::new(stream.try_clone()?);
    let request = match read_request(&mut reader) {
        Ok(request) => request,
        Err(RequestError::HeadersTooLarge) => {
            return write_error(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                "PLUGIN_FRAME_HEADERS_TOO_LARGE",
            )
        }
        Err(RequestError::Invalid) => {
            return write_error(
                &mut stream,
                400,
                "Bad Request",
                "PLUGIN_FRAME_REQUEST_INVALID",
            )
        }
        Err(RequestError::Io(error)) => return Err(error),
    };
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return write_error(
            &mut stream,
            405,
            "Method Not Allowed",
            "PLUGIN_ASSET_METHOD_NOT_ALLOWED",
        );
    }
    let expected_host = session.address.to_string();
    if request.header("host") != Some(expected_host.as_str()) {
        return write_error(&mut stream, 403, "Forbidden", "PLUGIN_FRAME_HOST_INVALID");
    }
    if let Some(origin) = request.header("origin") {
        if origin != session.origin {
            return write_error(&mut stream, 403, "Forbidden", "PLUGIN_FRAME_ORIGIN_INVALID");
        }
    }
    let path = match request_path(&request.target) {
        Ok(path) => path,
        Err(()) => {
            return write_error(&mut stream, 400, "Bad Request", "PLUGIN_ASSET_PATH_INVALID")
        }
    };
    let relative = match authorized_relative_path(&path, &request, session) {
        Ok(relative) => relative,
        Err(PathError::Unauthorized) => {
            return write_error(&mut stream, 403, "Forbidden", "PLUGIN_FRAME_TOKEN_INVALID")
        }
        Err(PathError::Invalid) => {
            return write_error(&mut stream, 400, "Bad Request", "PLUGIN_ASSET_PATH_INVALID")
        }
    };
    let asset = match (session.reader)(&relative) {
        Ok(asset) => asset,
        Err(error) => {
            let (status, reason) = status_for_error(&error);
            return write_error(&mut stream, status, reason, &error.code);
        }
    };
    let is_entry = relative == session.entry;
    let body = if is_entry {
        match inject_entry(&asset.bytes, session) {
            Ok(body) => body,
            Err(error) => {
                return write_error(&mut stream, 409, "Conflict", &error.code);
            }
        }
    } else {
        asset.bytes
    };
    let mut headers = vec![
        ("Content-Type", asset.content_type.to_string()),
        ("Content-Length", body.len().to_string()),
        ("Cache-Control", "no-store".to_string()),
        ("X-Content-Type-Options", "nosniff".to_string()),
        ("Access-Control-Allow-Origin", session.origin.clone()),
        ("Cross-Origin-Resource-Policy", "same-origin".to_string()),
        ("Referrer-Policy", "no-referrer".to_string()),
        ("Vary", "Origin".to_string()),
    ];
    if is_entry {
        headers.push(("Content-Security-Policy", entry_csp(session)));
        headers.push(("Set-Cookie", session_cookie(session)));
    }
    write_headers(&mut stream, 200, "OK", &headers)?;
    if request.method == "GET" {
        stream.write_all(&body)?;
    }
    drop(availability);
    Ok(())
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

enum RequestError {
    HeadersTooLarge,
    Invalid,
    Io(io::Error),
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Request, RequestError> {
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(RequestError::Io)?;
    let mut header_bytes = request_line.len();
    if header_bytes > MAX_REQUEST_HEADER_BYTES {
        return Err(RequestError::HeadersTooLarge);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(RequestError::Invalid)?.to_string();
    let target = parts.next().ok_or(RequestError::Invalid)?.to_string();
    let version = parts.next().ok_or(RequestError::Invalid)?;
    if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(RequestError::Invalid);
    }
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(RequestError::Io)?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        header_bytes += read;
        if header_bytes > MAX_REQUEST_HEADER_BYTES {
            return Err(RequestError::HeadersTooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(RequestError::Invalid)?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    Ok(Request {
        method,
        target,
        headers,
    })
}

fn request_path(target: &str) -> Result<String, ()> {
    if !target.starts_with('/') {
        return Err(());
    }
    let raw_path = target.split('?').next().ok_or(())?;
    let decoded = percent_decode_str(raw_path)
        .decode_utf8()
        .map(|path| path.into_owned())
        .map_err(|_| ())?;
    if decoded
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(());
    }
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| ())?;
    if url.path() != raw_path {
        return Err(());
    }
    Ok(decoded)
}

enum PathError {
    Unauthorized,
    Invalid,
}

fn authorized_relative_path(
    path: &str,
    request: &Request,
    session: &Session,
) -> Result<String, PathError> {
    let path = path.strip_prefix('/').ok_or(PathError::Invalid)?;
    let prefix = format!("{}/{}/", session.session_id, session.launch_token);
    let relative = if let Some(relative) = path.strip_prefix(&prefix) {
        relative
    } else if has_session_cookie(request, session) {
        path
    } else {
        return Err(PathError::Unauthorized);
    };
    if !is_safe_relative_path(relative) {
        return Err(PathError::Invalid);
    }
    Ok(relative.to_string())
}

fn has_session_cookie(request: &Request, session: &Session) -> bool {
    let expected = format!("{}={}", cookie_name(session), session.launch_token);
    request
        .header("cookie")
        .map(|cookies| cookies.split(';').any(|cookie| cookie.trim() == expected))
        .unwrap_or(false)
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn inject_entry(bytes: &[u8], session: &Session) -> Result<Vec<u8>, PluginCommandError> {
    let html = std::str::from_utf8(bytes).map_err(|_| {
        PluginCommandError::new(
            "PLUGIN_ENTRY_ENCODING_INVALID",
            "Plugin UI entry must be valid UTF-8",
        )
    })?;
    let content = strip_doctype(html);
    let nonce = &session.launch_token;
    let base = entry_base_url(session)?;
    let launch_token = serde_json::to_string(&session.launch_token).map_err(frame_server_error)?;
    let prefix = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{}\"><base href=\"{}\"><script nonce=\"{nonce}\">{}</script>",
        escape_attribute(&entry_csp_with_nonce(nonce)),
        escape_attribute(&base),
        bootstrap(&launch_token),
    );
    Ok(format!("<!doctype html>{prefix}{content}").into_bytes())
}

fn strip_doctype(html: &str) -> &str {
    let trimmed = html.trim_start();
    if trimmed
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<!doctype"))
    {
        return trimmed
            .find('>')
            .map(|offset| &trimmed[offset + 1..])
            .unwrap_or(trimmed);
    }
    html
}

fn entry_base_url(session: &Session) -> Result<String, PluginCommandError> {
    let mut base = url::Url::parse(&session_url(
        &session.origin,
        &session.session_id,
        &session.launch_token,
        &session.entry,
    )?)
    .map_err(frame_server_error)?;
    {
        let mut segments = base.path_segments_mut().map_err(|_| {
            PluginCommandError::new("PLUGIN_FRAME_URL_INVALID", "Plugin frame URL is invalid")
        })?;
        segments.pop();
        segments.push("");
    }
    Ok(base.to_string())
}

fn bootstrap(launch_token: &str) -> String {
    format!(
        "(()=>{{const launchToken={launch_token};const report=category=>parent.postMessage({{type:'attool:frame-error',launchToken,category}},'*');addEventListener('error',event=>report(event.target&&event.target!==window?'resource':'runtime'),true);addEventListener('unhandledrejection',()=>report('runtime'));addEventListener('vite:preloadError',()=>report('preload'));}})();"
    )
}

fn entry_csp(session: &Session) -> String {
    entry_csp_with_nonce(&session.launch_token)
}

fn entry_csp_with_nonce(nonce: &str) -> String {
    format!(
        "default-src 'none'; script-src 'self' 'nonce-{nonce}'; worker-src 'self' blob:; child-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' blob:; connect-src 'none'; object-src 'none'; frame-src 'none'; base-uri 'self'; form-action 'none'"
    )
}

fn cookie_name(session: &Session) -> String {
    format!("attool_frame_{}", session.session_id)
}

fn session_cookie(session: &Session) -> String {
    format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/",
        cookie_name(session),
        session.launch_token
    )
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn status_for_error(error: &PluginCommandError) -> (u16, &'static str) {
    match error.code.as_str() {
        "PLUGIN_DISABLED" => (403, "Forbidden"),
        "PLUGIN_NOT_FOUND" | "PLUGIN_ASSET_NOT_FOUND" | "PLUGIN_ASSET_NOT_DECLARED" => {
            (404, "Not Found")
        }
        "PLUGIN_ASSET_PATH_INVALID" => (400, "Bad Request"),
        "PLUGIN_ASSET_HASH_MISMATCH" => (409, "Conflict"),
        _ => (500, "Internal Server Error"),
    }
}

fn write_error(stream: &mut TcpStream, status: u16, reason: &str, code: &str) -> io::Result<()> {
    let body = code.as_bytes();
    write_headers(
        stream,
        status,
        reason,
        &[
            ("Content-Type", "text/plain; charset=utf-8".to_string()),
            ("Content-Length", body.len().to_string()),
            ("Content-Security-Policy", "default-src 'none'".to_string()),
            ("Cache-Control", "no-store".to_string()),
            ("X-Content-Type-Options", "nosniff".to_string()),
        ],
    )?;
    stream.write_all(body)
}

fn write_headers(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(impl AsRef<str>, String)],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    for (name, value) in headers {
        write!(stream, "{}: {value}\r\n", name.as_ref())?;
    }
    write!(stream, "Connection: close\r\n\r\n")?;
    stream.flush()
}

fn frame_server_error(error: impl std::fmt::Display) -> PluginCommandError {
    PluginCommandError::new(
        "PLUGIN_FRAME_SERVER_FAILED",
        format!("Plugin frame server failed: {error}"),
    )
}

fn session_lock_error() -> PluginCommandError {
    PluginCommandError::new(
        "PLUGIN_FRAME_SESSION_LOCK_FAILED",
        "Plugin frame session lock failed",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpStream,
        sync::Arc,
    };

    use super::{FrameAssetReader, FrameSessionRuntime};
    use crate::plugin::{commands::PluginCommandError, install::PluginAsset};

    fn reader() -> FrameAssetReader {
        let assets = HashMap::from([
            (
                "ui/index.html".to_string(),
                PluginAsset {
                    bytes: br#"<!doctype html><html><head></head><body><script type="module" src="./assets/app.js"></script></body></html>"#.to_vec(),
                    content_type: "text/html; charset=utf-8",
                },
            ),
            (
                "ui/assets/app.js".to_string(),
                PluginAsset {
                    bytes: b"globalThis.loaded = true;".to_vec(),
                    content_type: "text/javascript; charset=utf-8",
                },
            ),
        ]);
        Arc::new(move |relative| {
            assets.get(relative).cloned().ok_or_else(|| {
                PluginCommandError::new("PLUGIN_ASSET_NOT_DECLARED", "Plugin asset is not declared")
            })
        })
    }

    fn request(url: &str, method: &str, path: &str, headers: &[(&str, &str)]) -> Vec<u8> {
        let parsed = url::Url::parse(url).unwrap();
        let address = format!("{}:{}", parsed.host_str().unwrap(), parsed.port().unwrap());
        let mut stream = TcpStream::connect(&address).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\n").unwrap();
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").unwrap();
        }
        write!(stream, "\r\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    }

    fn route(url: &str) -> String {
        let parsed = url::Url::parse(url).unwrap();
        parsed.path().to_string()
    }

    fn response_text(response: &[u8]) -> &str {
        std::str::from_utf8(response).unwrap()
    }

    #[test]
    fn creates_a_unique_loopback_origin_for_each_frame() {
        let runtime = FrameSessionRuntime::default();
        let first = runtime.start("ui/index.html", reader()).unwrap();
        let second = runtime.start("ui/index.html", reader()).unwrap();

        let first_url = url::Url::parse(&first.url).unwrap();
        let second_url = url::Url::parse(&second.url).unwrap();
        assert_eq!(first_url.host_str(), Some("127.0.0.1"));
        assert_ne!(first_url.origin(), second_url.origin());
        assert_ne!(first.session_id, second.session_id);
        assert_ne!(first.launch_token, second.launch_token);

        let cross_session = request(&second.url, "GET", &route(&first.url), &[]);
        assert!(response_text(&cross_session).starts_with("HTTP/1.1 403 Forbidden\r\n"));
    }

    #[test]
    fn serves_entry_with_bootstrap_and_strict_security_headers() {
        let runtime = FrameSessionRuntime::default();
        let session = runtime.start("ui/index.html", reader()).unwrap();
        let response = request(&session.url, "GET", &route(&session.url), &[]);
        let response = response_text(&response);
        let expected_base = format!("{}/", session.url.rsplit_once('/').unwrap().0);

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Cache-Control: no-store\r\n"));
        assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(response.contains("Access-Control-Allow-Origin: http://127.0.0.1:"));
        assert!(response.contains("connect-src 'none'"));
        assert!(response.contains("worker-src 'self' blob:"));
        assert!(response.contains("attool:frame-error"));
        assert!(response.contains(&session.launch_token));
        assert!(response.contains(&format!("<base href=\"{expected_base}\">")));
        assert!(response.contains("src=\"./assets/app.js\""));
    }

    #[test]
    fn head_returns_the_same_resource_headers_without_a_body() {
        let runtime = FrameSessionRuntime::default();
        let session = runtime.start("ui/index.html", reader()).unwrap();
        let response = request(&session.url, "HEAD", &route(&session.url), &[]);
        let split = response
            .windows(4)
            .position(|value| value == b"\r\n\r\n")
            .unwrap();

        assert!(response_text(&response).starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(&response[split + 4..], b"");
    }

    #[test]
    fn serves_declared_assets_and_rejects_unauthorized_or_unsafe_requests() {
        let runtime = FrameSessionRuntime::default();
        let session = runtime.start("ui/index.html", reader()).unwrap();
        let entry = request(&session.url, "GET", &route(&session.url), &[]);
        let entry = response_text(&entry);
        let cookie = entry
            .lines()
            .find_map(|line| line.strip_prefix("Set-Cookie: "))
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let asset = request(
            &session.url,
            "GET",
            "/ui/assets/app.js",
            &[("Cookie", cookie)],
        );
        assert!(response_text(&asset).starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response_text(&asset).ends_with("globalThis.loaded = true;"));

        let missing_token = request(&session.url, "GET", "/ui/assets/app.js", &[]);
        assert!(response_text(&missing_token).starts_with("HTTP/1.1 403 Forbidden\r\n"));
        let traversal = request(&session.url, "GET", "/%2e%2e/secret", &[("Cookie", cookie)]);
        assert!(response_text(&traversal).starts_with("HTTP/1.1 400 Bad Request\r\n"));
        let undeclared = request(
            &session.url,
            "GET",
            "/ui/private.txt",
            &[("Cookie", cookie)],
        );
        assert!(response_text(&undeclared).starts_with("HTTP/1.1 404 Not Found\r\n"));
        let post = request(&session.url, "POST", &route(&session.url), &[]);
        assert!(response_text(&post).starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    }

    #[test]
    fn revoking_a_session_stops_serving_its_url_and_is_idempotent() {
        let runtime = FrameSessionRuntime::default();
        let session = runtime.start("ui/index.html", reader()).unwrap();

        assert!(runtime.revoke(&session.session_id));
        assert!(!runtime.revoke(&session.session_id));
        assert!(TcpStream::connect(
            url::Url::parse(&session.url)
                .unwrap()
                .socket_addrs(|| None)
                .unwrap()[0]
        )
        .is_err());
    }
}
