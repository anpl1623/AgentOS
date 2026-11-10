//! [`Egress::send`]: one request, made exactly as it was priced.
//!
//! The checks that need nothing but the request run first. Then the host is
//! resolved and the client pinned to what was checked, the credential is
//! looked up at the last moment, and the body is read past the caller's limit
//! only far enough to redact an echoed credential whole before it is cut.

use std::net::SocketAddr;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

use super::address::ip_literal;
use super::checks::{check_bound, check_header, origin_of};
use super::client::{client_for, request_failed};
use super::response::is_readable;
use super::{Egress, EgressRequest, EgressResponse, MAX_BODY_BYTES, Method, ResponseBody, Url};
use crate::error::ToolError;
use crate::tool::{CredentialResolver, Secret, ToolContext, redact_values};

impl Egress {
    /// Make one request.
    ///
    /// In order: the target and headers are checked, a credential bound to
    /// another origin is refused, the host is resolved and every address
    /// checked, the client is pinned to those addresses, the credential is
    /// resolved through `context` and attached, the request is sent once, and
    /// the body is read to the limit, redacted, and cut.
    ///
    /// Cancellation and the call's overall deadline are the caller's: race
    /// this against them, as any tool races the work it awaits.
    ///
    /// # Errors
    ///
    /// [`ToolError::Denied`] for an address the policy does not admit or a
    /// credential bound to another origin; [`ToolError::Failed`] for a target
    /// or header that cannot be sent, a credential that cannot be had, and a
    /// transport failure; [`ToolError::Io`] for a host that cannot be resolved.
    pub async fn send(
        &self,
        context: &ToolContext,
        request: EgressRequest,
    ) -> Result<EgressResponse, ToolError> {
        let origin = origin_of(&request.url)?;
        for (name, value) in &request.headers {
            check_header(name, value).map_err(ToolError::Failed)?;
        }
        if let Some(credential) = &request.credential {
            check_bound(credential, &origin)?;
            // A slice of a response is a slice of anything it echoes, and a
            // credential echoed a few bytes at a time, one range per request,
            // is never long enough in any one answer to be recognised.
            if let Some((range, _)) = request.headers.iter().find(|(name, _)| {
                name.eq_ignore_ascii_case("range") || name.eq_ignore_ascii_case("if-range")
            }) {
                return Err(ToolError::Failed(format!(
                    "header `{range}` cannot be set on a request that spends a credential: a \
                     slice of a response can carry a slice of the credential"
                )));
            }
        }

        let client = self.pinned_client(&request.url, request.timeout).await?;

        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            // Both were checked above; a failure here would be a bug, and is
            // reported rather than sending the request without them.
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            let value = HeaderValue::from_str(value)
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            headers.append(name, value);
        }
        let mut sent = None;
        if let Some(credential) = &request.credential {
            let (value, secret) = authorization(context, &origin, &credential.name).await?;
            headers.insert(AUTHORIZATION, value);
            sent = Some(secret);
        }

        let head = request.method == Method::HEAD;
        let mut builder = client.request(request.method, request.url).headers(headers);
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let mut response = builder
            .send()
            .await
            .map_err(|error| request_failed(&origin, &error))?;

        let status = response.status();
        let final_url = response.url().to_string();
        let echoed: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "set-cookie" | "set-cookie2"))
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());

        let cap = request.max_bytes.clamp(1, MAX_BODY_BYTES);
        let (body, truncated) = if head {
            (ResponseBody::None, false)
        } else if !is_readable(content_type.as_deref(), request.accept_diff) {
            let bytes = response.content_length();
            (
                ResponseBody::NotText { bytes },
                bytes.is_some_and(|bytes| bytes > 0),
            )
        } else if let Some(length) = response
            .content_length()
            .filter(|length| *length > u64::try_from(cap).unwrap_or(u64::MAX))
            .filter(|_| !request.accept_diff)
        {
            (ResponseBody::TooLarge { bytes: length, cap }, true)
        } else {
            // The cap is the caller's to choose, and a cut through the middle
            // of an echoed credential leaves a piece that no search for the
            // whole value finds. So the body is read past the cap by the
            // length of the credential this request sent, enough to hold it
            // whole wherever it starts before the cut; it is redacted there,
            // and only then cut. The pipeline redacts the result again against
            // everything the run has released, pieces included, for a token
            // an earlier call sent and this one is handed back.
            let margin = sent
                .as_ref()
                .map_or(0, |secret: &Secret| secret.expose().len());
            let limit = cap.saturating_add(margin);
            let mut read = Vec::new();
            let mut more = false;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| request_failed(&origin, &error))?
            {
                let room = limit - read.len();
                if chunk.len() > room {
                    read.extend_from_slice(&chunk[..room]);
                    more = true;
                    break;
                }
                read.extend_from_slice(&chunk);
            }
            let mut text = String::from_utf8_lossy(&read).into_owned();
            if let Some(secret) = &sent
                && let Some(redacted) = redact_values(&text, &[secret.expose()])
            {
                text = redacted;
            }
            let truncated = more || text.len() > cap;
            if text.len() > cap {
                let mut cut = cap;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
            }
            (ResponseBody::Text(text), truncated)
        };

        Ok(EgressResponse {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or("").to_owned(),
            headers: echoed,
            body,
            truncated,
            final_url,
            content_type,
        })
    }

    /// A client that can reach the checked addresses of `url`'s host and
    /// nothing else.
    async fn pinned_client(
        &self,
        url: &Url,
        timeout: Duration,
    ) -> Result<reqwest::Client, ToolError> {
        let host = url
            .host_str()
            .ok_or_else(|| ToolError::Failed(format!("`{url}` has no host")))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| ToolError::Failed(format!("`{url}` has no port")))?;

        let addresses: Vec<SocketAddr> = if let Some(literal) = ip_literal(host) {
            // The literal is the address; there is no name for an answer to
            // change. The resolver below is installed anyway, so that nothing
            // this client does can reach the system's.
            vec![SocketAddr::new(literal, port)]
        } else {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|source| ToolError::io(format!("resolving `{host}`"), source))?
                .collect()
        };
        if addresses.is_empty() {
            return Err(ToolError::Failed(format!(
                "`{host}` resolved to no address"
            )));
        }
        for address in &addresses {
            if let Some(kind) = self.refusal(address.ip()) {
                return Err(ToolError::Denied {
                    reason: format!(
                        "refusing to connect to `{host}`: it reaches {}, which is {kind}, \
                         not a public address",
                        address.ip()
                    ),
                });
            }
        }
        client_for(host, addresses, timeout)
    }
}

/// The `Authorization` header for credential `name` on `origin`, and the
/// value it carries.
///
/// The value is looked up here, at the moment of sending. The header is marked
/// sensitive so the HTTP stack neither indexes nor prints it, and the value is
/// handed back only so the response can be redacted against it before it is
/// cut short.
async fn authorization(
    context: &ToolContext,
    origin: &str,
    name: &str,
) -> Result<(HeaderValue, Secret), ToolError> {
    let resolver: &dyn CredentialResolver = context.credentials.as_deref().ok_or_else(|| {
        ToolError::Failed(format!(
            "credential `{name}` cannot be used: this run has no credential store"
        ))
    })?;
    let secret = resolver.resolve(origin, name).await.ok_or_else(|| {
        ToolError::Failed(format!(
            "no credential named `{name}` is stored for {origin}; a credential is bound to \
             the origin it was stored for"
        ))
    })?;
    let mut value =
        HeaderValue::from_str(&format!("Bearer {}", secret.expose())).map_err(|_| {
            ToolError::Failed(format!(
                "credential `{name}` contains characters a header cannot carry"
            ))
        })?;
    value.set_sensitive(true);
    Ok((value, secret))
}
