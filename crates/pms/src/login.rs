use std::time::Duration;

use anyhow::{bail, Result};

use crate::session::StoredSession;
use crate::supabase::{Supabase, SupabaseConfig};

#[derive(serde::Deserialize)]
struct DeviceStart {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(serde::Deserialize)]
struct DeviceSession {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    supabase_url: String,
    email: String,
    user_id: String,
}

pub async fn device_login(auth_url: &str, open_browser: bool) -> Result<StoredSession> {
    let http = reqwest::Client::new();
    let start: DeviceStart = http.post(format!("{}/device", auth_url.trim_end_matches('/')))
        .json(&serde_json::json!({"client_info": format!("pms {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS)}))
        .send().await?.error_for_status()?.json().await?;
    eprintln!(
        "Open {} and enter code: {}",
        start.verification_uri, start.user_code
    );
    if open_browser && open::that(&start.verification_uri_complete).is_err() {
        eprintln!("Could not open the browser. Open the URL above yourself.");
    }
    eprintln!("Waiting for sign-in...");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(start.expires_in);
    let mut interval = start.interval.max(1);
    loop {
        if tokio::time::Instant::now() >= deadline {
            bail!("Device login expired. Run `pms login` again.");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let response = http
            .post(format!("{}/device/token", auth_url.trim_end_matches('/')))
            .json(&serde_json::json!({"device_code": start.device_code}))
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        if status == reqwest::StatusCode::ACCEPTED {
            continue;
        }
        if status.is_success() {
            let session: DeviceSession = serde_json::from_value(body)?;
            return Ok(StoredSession {
                access_token: session.access_token,
                refresh_token: session.refresh_token,
                expires_at: session.expires_at,
                supabase_url: session.supabase_url,
                email: session.email,
                user_id: session.user_id,
                provider: "github".into(),
            });
        }
        match body
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
        {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += 5;
                continue;
            }
            "access_denied" => bail!("Device login was denied."),
            "expired_token" => bail!("Device login expired. Run `pms login` again."),
            _ => bail!("Device login failed ({status}): {body}"),
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
mod flow_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn device_flow_polls_and_returns_new_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/device"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_code":"opaque","user_code":"BCDF-GHJK","verification_uri":"https://auth.example/",
                "verification_uri_complete":"https://auth.example/BCDF-GHJK","expires_in":30,"interval":1
            }))).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/device/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token":"access","refresh_token":"refresh","expires_at":1234,
                "supabase_url":"https://project.example","email":"user@example.com","user_id":"user-1"
            }))).expect(1).mount(&server).await;
        let session = device_login(&server.uri(), false).await.unwrap();
        assert_eq!(session.access_token, "access");
        assert_eq!(session.refresh_token, "refresh");
        assert_eq!(session.supabase_url, "https://project.example");
        assert_eq!(session.email, "user@example.com");
    }
}
