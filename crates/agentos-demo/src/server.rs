//! A minimal HTTP server for the mock CRM.
//!
//! Hand-rolled rather than pulling in a web framework. It serves five static
//! page shapes to one browser on loopback, and that does not justify a
//! dependency tree the security-sensitive parts of this project would then also
//! have to carry.
//!
//! It is not a general-purpose server and makes no attempt to be one: no
//! keep-alive, no compression, no TLS, loopback only.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::crm;

/// Largest request AgentOS will read before giving up on it.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

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
                            tokio::spawn(async move {
                                if let Err(error) = serve(stream).await {
                                    tracing::debug!(%error, "mock CRM connection ended");
                                }
                            });
                        }
                        Err(error) => {
                            tracing::warn!(%error, "mock CRM accept failed");
                            break;
                        }
                    },
                }
            }
        });

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

async fn serve(mut stream: TcpStream) -> io::Result<()> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];

    // Read until the end of the headers. The mock CRM has no request bodies, so
    // there is nothing after them to wait for.
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return respond(&mut stream, 431, "text/plain", "Request header too large").await;
        }
    }

    let request = String::from_utf8_lossy(&buffer);
    let Some(line) = request.lines().next() else {
        return respond(&mut stream, 400, "text/plain", "Bad request").await;
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");

    if !matches!(method, "GET" | "HEAD") {
        return respond(&mut stream, 405, "text/plain", "Method not allowed").await;
    }

    let path = target.split('?').next().unwrap_or("/");
    let (status, body) = route(path);
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

async fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
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
