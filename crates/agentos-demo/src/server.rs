//! A minimal HTTP server for the mock CRM and the mock GitHub.
//!
//! Hand-rolled rather than pulling in a web framework. Between them the two
//! mocks serve a handful of fixed shapes to one client on loopback, and that
//! does not justify a dependency tree the security-sensitive parts of this
//! project would then also have to carry.
//!
//! It is not a general-purpose server and makes no attempt to be one: no
//! keep-alive, no chunked bodies, no compression, no TLS, loopback only.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::crm;

/// Largest request AgentOS will read before giving up on it, head and body
/// each.
pub(crate) const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// A running mock CRM.
#[derive(Debug)]
pub struct MockCrm {
    base_url: String,
    address: SocketAddr,
    shutdown: Arc<Notify>,
}

impl MockCrm {
    /// Start the server on a loopback port chosen by the operating system.
    ///
    /// Binding to port 0 means several tests can run at once without agreeing on
    /// a port, and binding to 127.0.0.1 rather than 0.0.0.0 means the mock CRM
    /// is not reachable from the network.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the port cannot be bound.
    pub async fn start() -> io::Result<Self> {
        let (address, shutdown) = listen("mock CRM", serve).await?;
        Ok(Self {
            base_url: format!("http://127.0.0.1:{}", address.port()),
            address,
            shutdown,
        })
    }

    /// The base URL, e.g. `http://127.0.0.1:52341`.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// A URL for a path on the mock CRM.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// The bound address.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Stop accepting connections.
    pub fn stop(&self) {
        self.shutdown.notify_waiters();
    }
}

impl Drop for MockCrm {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Accept connections on a fresh loopback port and hand each to `handle`,
/// until the returned signal is notified.
///
/// The one place either mock binds a socket, so the one place to check that
/// neither is reachable off the machine.
pub(crate) async fn listen<H, F>(
    name: &'static str,
    handle: H,
) -> io::Result<(SocketAddr, Arc<Notify>)>
where
    H: Fn(TcpStream) -> F + Send + Sync + 'static,
    F: Future<Output = io::Result<()>> + Send + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let shutdown = Arc::new(Notify::new());
    let signal = shutdown.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = signal.notified() => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, _peer)) => {
                        let connection = handle(stream);
                        tokio::spawn(async move {
                            if let Err(error) = connection.await {
                                tracing::debug!(%error, "{name} connection ended");
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "{name} accept failed");
                        break;
                    }
                },
            }
        }
    });

    Ok((address, shutdown))
}

/// One request, as read off the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    /// The method, as sent.
    pub(crate) method: String,
    /// The request target: path and query.
    pub(crate) target: String,
    /// Header names, lowercased, and their values.
    pub(crate) headers: Vec<(String, String)>,
    /// The body, when `Content-Length` declared one.
    pub(crate) body: Vec<u8>,
}

impl Request {
    /// The path, without the query.
    pub(crate) fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("/")
    }

    /// The query, without the `?`.
    pub(crate) fn query(&self) -> Option<&str> {
        self.target.split_once('?').map(|(_, query)| query)
    }

    /// A header's value, by case-insensitive name.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// What reading a request came to.
#[derive(Debug)]
pub(crate) enum Read {
    /// A whole request.
    Request(Request),
    /// Something that must be refused with this status and text.
    Refused(u16, &'static str),
}

/// Read one request: the head, then as much body as `Content-Length` says.
///
/// Neither part may exceed [`MAX_REQUEST_BYTES`]. A body declared longer is
/// refused before any of it is read, rather than read and then refused.
pub(crate) async fn read_request(stream: &mut TcpStream) -> io::Result<Read> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];

    let end = loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Ok(Read::Refused(431, "Request header too large"));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            // A client that hung up mid-head gets an answer it will not read,
            // which is simpler than a third outcome every caller handles.
            return Ok(Read::Refused(400, "Bad request"));
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(Read::Refused(400, "Bad request"));
    };
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();

    let declared = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse::<usize>());
    let length = match declared {
        None => 0,
        Some(Ok(length)) if length <= MAX_REQUEST_BYTES => length,
        Some(Ok(_)) => return Ok(Read::Refused(413, "Request body too large")),
        Some(Err(_)) => return Ok(Read::Refused(400, "Bad request")),
    };
    let mut body = buffer.split_off(end + 4);
    while body.len() < length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(Read::Refused(400, "Bad request"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);

    Ok(Read::Request(Request {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
        body,
    }))
}

async fn serve(mut stream: TcpStream) -> io::Result<()> {
    let request = match read_request(&mut stream).await? {
        Read::Request(request) => request,
        Read::Refused(status, text) => {
            return respond(&mut stream, status, "text/plain", text).await;
        }
    };

    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return respond(&mut stream, 405, "text/plain", "Method not allowed").await;
    }

    let (status, body) = route(request.path());
    respond(&mut stream, status, "text/html; charset=utf-8", &body).await
}

/// Map a path to a page.
fn route(path: &str) -> (u16, String) {
    let trimmed = path.trim_end_matches('/');
    match trimmed {
        "" => (200, crm::dashboard()),
        "/customers" => (200, crm::customer_list()),
        "/favicon.ico" => (404, String::new()),
        _ => {
            for (prefix, render) in [("/customers/", 0u8), ("/compose/", 1), ("/sent/", 2)] {
                if let Some(id) = trimmed.strip_prefix(prefix) {
                    return match crm::customer(id) {
                        None => (404, crm::not_found(path)),
                        Some(customer) => (
                            200,
                            match render {
                                0 => crm::customer_detail(customer),
                                1 => crm::compose(customer),
                                _ => crm::sent(customer),
                            },
                        ),
                    };
                }
            }
            (404, crm::not_found(path))
        }
    }
}

/// Write a whole response and let the connection close.
pub(crate) async fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        422 => "Unprocessable Entity",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {len}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{body}",
        len = body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_covers_every_page() {
        assert_eq!(route("/").0, 200);
        assert_eq!(route("").0, 200);
        assert_eq!(route("/customers").0, 200);
        assert_eq!(route("/customers/").0, 200);
        assert_eq!(route("/customers/acme").0, 200);
        assert_eq!(route("/compose/acme").0, 200);
        assert_eq!(route("/sent/acme").0, 200);
    }

    #[test]
    fn unknown_paths_and_customers_are_not_found() {
        assert_eq!(route("/nope").0, 404);
        assert_eq!(route("/customers/nobody").0, 404);
        assert_eq!(route("/compose/nobody").0, 404);
    }

    #[test]
    fn the_customer_page_carries_the_planted_note() {
        let (status, body) = route("/customers/globex");
        assert_eq!(status, 200);
        assert!(body.contains("SYSTEM NOTICE"));
    }

    #[tokio::test]
    async fn the_server_binds_to_loopback_and_serves_pages() {
        let crm = MockCrm::start().await.unwrap();
        assert!(
            crm.address().ip().is_loopback(),
            "must not be reachable off-host"
        );
        assert!(crm.base_url().starts_with("http://127.0.0.1:"));

        let body = fetch(&crm, "/customers").await;
        assert!(body.contains("HTTP/1.1 200 OK"));
        assert!(body.contains("Acme Corporation"));
    }

    #[tokio::test]
    async fn unknown_paths_return_404_over_the_wire() {
        let crm = MockCrm::start().await.unwrap();
        let body = fetch(&crm, "/definitely-not-here").await;
        assert!(body.contains("HTTP/1.1 404 Not Found"));
    }

    #[tokio::test]
    async fn non_get_methods_are_refused() {
        let crm = MockCrm::start().await.unwrap();
        let mut stream = TcpStream::connect(crm.address()).await.unwrap();
        stream
            .write_all(b"DELETE /customers HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .unwrap();
        assert!(response.contains("405"));
    }

    #[tokio::test]
    async fn a_body_declared_over_the_limit_is_refused_unread() {
        let crm = MockCrm::start().await.unwrap();
        let mut stream = TcpStream::connect(crm.address()).await.unwrap();
        stream
            .write_all(
                format!(
                    "POST /customers HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
                    MAX_REQUEST_BYTES + 1
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    }

    async fn fetch(crm: &MockCrm, path: &str) -> String {
        let mut stream = TcpStream::connect(crm.address()).await.unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .unwrap();
        response
    }
}
