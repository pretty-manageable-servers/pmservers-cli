mod common;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use pms_client::{Client, Error, TokenSource};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn me_sends_bearer_and_parses() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90", "email": "a@b.co", "plan": "free"}),
        ))
        .mount(&server)
        .await;
    let me = client.me().await.unwrap();
    assert_eq!(me.email, "a@b.co");
    assert_eq!(me.plan, "free");
}

#[tokio::test]
async fn catalog_parses_all_lists() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/catalog"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "regions": ["ca-tor-1"],
            "container_sizes": [{"id": "pms-c-250m-512", "vcpu": 0.25, "memory_mib": 512}],
            "database_sizes": [{"id": "pms-db-250m-512", "vcpu": 0.25, "memory_mib": 512}],
            "ai_models": [{"id": "gemma4-e4b", "type": "llm", "description": "d", "detail": "x"}]
        })))
        .mount(&server)
        .await;
    let c = client.catalog().await.unwrap();
    assert_eq!(c.regions, ["ca-tor-1"]);
    assert_eq!(c.container_sizes[0].vcpu, 0.25);
    assert_eq!(c.ai_models[0].kind, "llm");
}

#[tokio::test]
async fn api_error_body_becomes_error() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(ResponseTemplate::new(404).set_body_json(
            json!({"error": {"code": "not_found", "message": "Project not found."}}),
        ))
        .mount(&server)
        .await;
    let err = client.me().await.unwrap_err();
    assert!(err.is_not_found());
    assert_eq!(err.code(), Some("not_found"));
    assert_eq!(err.to_string(), "Project not found.");
}

#[tokio::test]
async fn html_error_page_is_not_shown() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>"))
        .mount(&server)
        .await;
    let err = client.me().await.unwrap_err();
    assert_eq!(err.status(), Some(502));
    assert!(!err.to_string().contains('<'));
}

struct Rotating {
    refreshes: AtomicUsize,
}

#[async_trait]
impl TokenSource for Rotating {
    async fn access_token(&self) -> pms_client::Result<String> {
        Ok("old".into())
    }
    async fn refresh(&self) -> pms_client::Result<String> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Ok("new".into())
    }
}

#[tokio::test]
async fn refreshes_once_on_401_and_retries() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer old"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(
                json!({"error": {"code": "unauthorized", "message": "Sign in again."}}),
            ),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer new"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "u", "email": "e", "plan": "free"})),
        )
        .mount(&server)
        .await;
    let tokens = Arc::new(Rotating {
        refreshes: AtomicUsize::new(0),
    });
    let client = Client::new(&server.uri(), tokens.clone()).unwrap();
    client.me().await.unwrap();
    assert_eq!(tokens.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn second_401_is_an_error_not_a_loop() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(
                json!({"error": {"code": "unauthorized", "message": "Sign in again."}}),
            ),
        )
        .mount(&server)
        .await;
    let tokens = Arc::new(Rotating {
        refreshes: AtomicUsize::new(0),
    });
    let client = Client::new(&server.uri(), tokens.clone()).unwrap();
    let err = client.me().await.unwrap_err();
    assert!(matches!(err, Error::Api { status: 401, .. }));
    assert_eq!(tokens.refreshes.load(Ordering::SeqCst), 1);
}
