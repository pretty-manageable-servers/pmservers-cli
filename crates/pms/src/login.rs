use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::session::StoredSession;
use crate::supabase::{Pkce, Supabase, SupabaseConfig};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Failed(String),
}

/// Read one HTTP request and return the callback, or None for any other path (a favicon request).
pub async fn read_callback(stream: &mut TcpStream) -> Result<Option<Callback>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 8192 {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("GET "))
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("");
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    let (status, body, result) = if url.path() == "/callback" {
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        if let Some(code) = q.get("code") {
            (
                "200 OK",
                "You are signed in. You can close this tab.",
                Some(Callback::Code(code.clone())),
            )
        } else {
            let why = q
                .get("error_description")
                .or_else(|| q.get("error"))
                .cloned()
                .unwrap_or_else(|| "no code in the callback".into());
            (
                "400 Bad Request",
                "Sign-in failed. Go back to the terminal.",
                Some(Callback::Failed(why)),
            )
        }
    } else {
        ("404 Not Found", "Not found.", None)
    };
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>pms</title><body style=\"font-family:sans-serif\"><p>{body}</p>"
    );
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
    Ok(result)
}

pub async fn wait_for_callback(listener: &TcpListener) -> Result<Callback> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        if let Some(cb) = read_callback(&mut stream).await.unwrap_or(None) {
            return Ok(cb);
        }
    }
}

pub async fn browser_login(
    cfg: SupabaseConfig,
    port: Option<u16>,
    open_browser: bool,
) -> Result<StoredSession> {
    let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
        .await
        .with_context(|| match port {
            Some(p) => format!("cannot listen on 127.0.0.1:{p}"),
            None => "cannot listen on 127.0.0.1".to_owned(),
        })?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let pkce = Pkce::new();
    let supabase_url = cfg.url.clone();
    let supabase = Supabase::new(cfg);
    let url = supabase.authorize_url(&redirect, &pkce.challenge)?;

    eprintln!("Open this URL in your browser to sign in with GitHub:\n\n  {url}\n");
    if open_browser && open::that(url.as_str()).is_err() {
        eprintln!("Could not open the browser. Open the URL yourself.");
    }
    eprintln!("Waiting for the sign-in (5 minutes)...");

    let callback = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_callback(&listener))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Timed out. If your browser went to the console instead of {redirect}, \
                 Supabase does not allow this redirect URL. Use `pms login --port <port>` \
                 with a URL that is on the allow list, or `pms login --email`."
            )
        })??;
    match callback {
        Callback::Failed(why) => bail!("sign-in failed: {why}"),
        Callback::Code(code) => {
            let resp = supabase.exchange_code(&code, &pkce.verifier).await?;
            Ok(StoredSession::from_response(&resp, &supabase_url))
        }
    }
}

pub async fn password_login(
    cfg: SupabaseConfig,
    email: &str,
    password: &str,
) -> Result<StoredSession> {
    let supabase_url = cfg.url.clone();
    let resp = Supabase::new(cfg).password(email, password).await?;
    Ok(StoredSession::from_response(&resp, &supabase_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn request(path: &str) -> (Option<Callback>, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let path = path.to_owned();
        let client = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        let cb = read_callback(&mut stream).await.unwrap();
        (cb, client.await.unwrap())
    }

    #[tokio::test]
    async fn callback_with_code() {
        let (cb, resp) = request("/callback?code=abc%20d").await;
        assert_eq!(cb, Some(Callback::Code("abc d".into())));
        assert!(resp.starts_with("HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn callback_with_error() {
        let (cb, resp) = request("/callback?error=access_denied&error_description=No+way").await;
        assert_eq!(cb, Some(Callback::Failed("No way".into())));
        assert!(resp.starts_with("HTTP/1.1 400"));
    }

    #[tokio::test]
    async fn other_paths_are_ignored() {
        let (cb, resp) = request("/favicon.ico").await;
        assert_eq!(cb, None);
        assert!(resp.starts_with("HTTP/1.1 404"));
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn browser_flow_exchanges_the_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(query_param("grant_type", "pkce"))
            .and(body_partial_json(json!({"auth_code": "abc"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "a", "refresh_token": "r", "expires_in": 3600,
                "user": {"id": "u1", "email": "x@y.zz", "app_metadata": {"provider": "github"}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let cfg = SupabaseConfig {
            url: server.uri(),
            anon_key: "anon".into(),
        };
        let flow = tokio::spawn(browser_login(cfg, Some(port), false));
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => break s,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        stream
            .write_all(b"GET /callback?code=abc HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut sink = String::new();
        stream.read_to_string(&mut sink).await.unwrap();
        let session = flow.await.unwrap().unwrap();
        assert_eq!(session.provider, "github");
        assert_eq!(session.email, "x@y.zz");
        assert_eq!(session.supabase_url, server.uri());
    }
}
