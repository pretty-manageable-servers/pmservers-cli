use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use url::Url;

use crate::error::{Error, Result};

/// Where the client gets its bearer token. The CLI implements it with a Supabase session.
/// Keep it behind this trait so that other token kinds can be added later.
#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn access_token(&self) -> Result<String>;
    /// Called once after a 401. Return a new token, or an error if the session is over.
    async fn refresh(&self) -> Result<String>;
    /// False for a token that cannot be renewed (an API token). A 401 then shows the API's message.
    fn can_refresh(&self) -> bool {
        true
    }
}

/// A token that cannot be renewed, such as an API token from `PMS_TOKEN`.
pub struct StaticToken(pub String);

#[async_trait]
impl TokenSource for StaticToken {
    async fn access_token(&self) -> Result<String> {
        Ok(self.0.clone())
    }

    fn can_refresh(&self) -> bool {
        false
    }

    async fn refresh(&self) -> Result<String> {
        Err(Error::Auth("The token was refused.".into()))
    }
}

#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) base: Url,
    tokens: Arc<dyn TokenSource>,
}

impl Client {
    pub fn new(base_url: &str, tokens: Arc<dyn TokenSource>) -> Result<Self> {
        let base = Url::parse(base_url)
            .map_err(|e| Error::Invalid(format!("bad API URL {base_url:?}: {e}")))?;
        if base.cannot_be_a_base() {
            return Err(Error::Invalid(format!("bad API URL {base_url:?}")));
        }
        let http = reqwest::Client::builder()
            .user_agent(concat!("pms/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self { http, base, tokens })
    }

    /// Build a URL from path segments. Each segment is escaped.
    pub fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().expect("checked in new");
            path.pop_if_empty().extend(segments);
        }
        url
    }

    pub async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<serde_json::Value>,
    ) -> Result<Vec<u8>> {
        let mut token = self.tokens.access_token().await?;
        let mut retried = false;
        loop {
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(&token);
            if let Some(body) = &body {
                req = req.json(body);
            }
            let resp = req.send().await?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED && !retried && self.tokens.can_refresh() {
                retried = true;
                token = self.tokens.refresh().await?;
                continue;
            }
            let bytes = resp.bytes().await?.to_vec();
            if status.is_success() {
                return Ok(bytes);
            }
            return Err(api_error(status, &bytes));
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, segments: &[&str]) -> Result<T> {
        decode(&self.send(Method::GET, self.url(segments), None).await?)
    }

    pub async fn get_query<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<T> {
        let mut url = self.url(segments);
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        decode(&self.send(Method::GET, url, None).await?)
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        segments: &[&str],
        body: &B,
    ) -> Result<T> {
        decode(
            &self
                .send(Method::POST, self.url(segments), Some(to_value(body)?))
                .await?,
        )
    }

    pub async fn post_empty<T: DeserializeOwned>(&self, segments: &[&str]) -> Result<T> {
        decode(&self.send(Method::POST, self.url(segments), None).await?)
    }

    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        segments: &[&str],
        body: &B,
    ) -> Result<T> {
        decode(
            &self
                .send(Method::PATCH, self.url(segments), Some(to_value(body)?))
                .await?,
        )
    }

    /// For routes that answer 202 or 204 with no body.
    pub async fn post_unit<B: Serialize>(&self, segments: &[&str], body: &B) -> Result<()> {
        self.send(Method::POST, self.url(segments), Some(to_value(body)?))
            .await
            .map(drop)
    }

    pub async fn delete(&self, segments: &[&str]) -> Result<()> {
        self.send(Method::DELETE, self.url(segments), None)
            .await
            .map(drop)
    }
}

fn to_value<B: Serialize>(body: &B) -> Result<serde_json::Value> {
    serde_json::to_value(body).map_err(|e| Error::Invalid(e.to_string()))
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| Error::Decode(e.to_string()))
}

fn api_error(status: StatusCode, body: &[u8]) -> Error {
    #[derive(serde::Deserialize)]
    struct Body {
        error: Inner,
    }
    #[derive(serde::Deserialize)]
    struct Inner {
        code: String,
        message: String,
    }
    match serde_json::from_slice::<Body>(body) {
        Ok(b) => Error::Api {
            status: status.as_u16(),
            code: b.error.code,
            message: b.error.message,
        },
        Err(_) => Error::Api {
            status: status.as_u16(),
            code: "unknown".into(),
            // A proxy can answer with HTML. Show only the status then.
            message: format!(
                "The API answered {}.",
                status.canonical_reason().unwrap_or(status.as_str())
            ),
        },
    }
}
