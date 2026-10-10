use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

/// The URL and anon key are public values. A release build gets them from the environment at
/// build time. A runtime variable with the same name wins.
#[derive(Debug, Clone)]
pub struct SupabaseConfig {
    pub url: String,
    pub anon_key: String,
}

impl SupabaseConfig {
    pub fn resolve() -> Result<Self> {
        Self::from_sources(
            std::env::var("PMS_SUPABASE_URL").ok(),
            std::env::var("PMS_SUPABASE_ANON_KEY").ok(),
            option_env!("PMS_SUPABASE_URL"),
            option_env!("PMS_SUPABASE_ANON_KEY"),
        )
    }

    /// For a saved session: the session knows its Supabase URL. Only the anon key comes from the build or the environment.
    pub fn for_session(url: &str) -> Result<Self> {
        let key = std::env::var("PMS_SUPABASE_ANON_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| option_env!("PMS_SUPABASE_ANON_KEY").map(str::to_owned))
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| anyhow!("This build of pms has no Supabase anon key. Set PMS_SUPABASE_ANON_KEY in the environment."))?;
        Ok(Self {
            url: url.to_owned(),
            anon_key: key.trim().to_owned(),
        })
    }

    fn from_sources(
        env_url: Option<String>,
        env_key: Option<String>,
        built_url: Option<&str>,
        built_key: Option<&str>,
    ) -> Result<Self> {
        let pick = |runtime: Option<String>, built: Option<&str>| {
            runtime
                .filter(|v| !v.trim().is_empty())
                .or_else(|| built.filter(|v| !v.trim().is_empty()).map(str::to_owned))
        };
        match (pick(env_url, built_url), pick(env_key, built_key)) {
            (Some(url), Some(anon_key)) => Ok(Self {
                url: url.trim().trim_end_matches('/').to_owned(),
                anon_key: anon_key.trim().to_owned(),
            }),
            _ => bail!(
                "This build of pms has no Supabase settings. Set PMS_SUPABASE_URL and \
                 PMS_SUPABASE_ANON_KEY in the environment."
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SbUser {
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub app_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub expires_at: Option<i64>,
    pub user: SbUser,
}

impl TokenResponse {
    pub fn expires_at(&self) -> i64 {
        self.expires_at
            .unwrap_or_else(|| crate::session::now() + self.expires_in)
    }
}

pub struct Supabase {
    http: reqwest::Client,
    cfg: SupabaseConfig,
}

impl Supabase {
    pub fn new(cfg: SupabaseConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            cfg,
        }
    }

    pub async fn password(&self, email: &str, password: &str) -> Result<TokenResponse> {
        self.token(
            "password",
            serde_json::json!({"email": email, "password": password}),
        )
        .await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse> {
        self.token(
            "refresh_token",
            serde_json::json!({"refresh_token": refresh_token}),
        )
        .await
    }

    /// `scope=local` ends this session only. The default scope would end the console session too.
    pub async fn logout(&self, access_token: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/auth/v1/logout?scope=local", self.cfg.url))
            .header("apikey", &self.cfg.anon_key)
            .bearer_auth(access_token)
            .send()
            .await?;
        // 401 means the session is already gone.
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(());
        }
        Err(error_from(resp).await)
    }

    async fn token(&self, grant: &str, body: serde_json::Value) -> Result<TokenResponse> {
        let resp = self
            .http
            .post(format!("{}/auth/v1/token?grant_type={grant}", self.cfg.url))
            .header("apikey", &self.cfg.anon_key)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(error_from(resp).await);
        }
        resp.json()
            .await
            .map_err(|e| anyhow!("unexpected sign-in response: {e}"))
    }
}

async fn error_from(resp: reqwest::Response) -> anyhow::Error {
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let text = ["error_description", "msg", "message", "error"]
        .iter()
        .find_map(|k| body.get(k).and_then(|v| v.as_str()));
    match text {
        Some(t) => anyhow!("sign-in failed: {t}"),
        None => anyhow!("sign-in failed: the auth server answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_env_beats_build_value() {
        let c = SupabaseConfig::from_sources(
            Some("https://run.example/".into()),
            None,
            Some("https://built.example"),
            Some("key"),
        )
        .unwrap();
        assert_eq!(c.url, "https://run.example");
        assert_eq!(c.anon_key, "key");
    }

    #[test]
    fn missing_settings_are_an_error() {
        assert!(SupabaseConfig::from_sources(None, None, None, None).is_err());
        assert!(
            SupabaseConfig::from_sources(Some("".into()), Some("k".into()), None, None).is_err()
        );
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tokens() -> serde_json::Value {
        json!({"access_token": "a", "refresh_token": "r", "expires_in": 3600, "expires_at": 1893456000,
               "user": {"id": "u1", "email": "x@y.zz", "app_metadata": {"provider": "email"}}})
    }

    async fn server() -> (MockServer, Supabase) {
        let server = MockServer::start().await;
        let s = Supabase::new(SupabaseConfig {
            url: server.uri(),
            anon_key: "anon".into(),
        });
        (server, s)
    }

    #[tokio::test]
    async fn password_grant() {
        let (server, s) = server().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(query_param("grant_type", "password"))
            .and(header("apikey", "anon"))
            .and(body_json(json!({"email": "x@y.zz", "password": "pw"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens()))
            .mount(&server)
            .await;
        let t = s.password("x@y.zz", "pw").await.unwrap();
        assert_eq!(t.expires_at(), 1893456000);
        assert_eq!(t.user.email.as_deref(), Some("x@y.zz"));
    }

    #[tokio::test]
    async fn bad_password_shows_the_server_message() {
        let (server, s) = server().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                json!({"code": 400, "error_code": "invalid_credentials", "msg": "Invalid login credentials"}),
            ))
            .mount(&server)
            .await;
        let err = s.password("x@y.zz", "bad").await.unwrap_err().to_string();
        assert_eq!(err, "sign-in failed: Invalid login credentials");
    }

    #[tokio::test]
    async fn oauth_style_error_is_read() {
        let (server, s) = server().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                json!({"error": "invalid_grant", "error_description": "Refresh Token Not Found"}),
            ))
            .mount(&server)
            .await;
        let err = s.refresh("old").await.unwrap_err().to_string();
        assert!(err.contains("Refresh Token Not Found"));
    }

    #[tokio::test]
    async fn logout_treats_401_as_done() {
        let (server, s) = server().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/logout"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        s.logout("tok").await.unwrap();
    }
}
