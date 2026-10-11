mod common;

use pms_client::tokens::CreatedApiToken;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

const TOKEN: &str = "pms_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";

fn token(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({"id": "tok_abc123", "name": "ci", "prefix": "pms_0123",
                       "created_at": "2026-10-11T10:00:00+00:00", "expires_at": null,
                       "last_used_at": null});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[tokio::test]
async fn list_tokens_reads_items() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/tokens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            token(json!({})),
            token(json!({"id": "tok_def456", "name": "deploy", "expires_at": "2027-01-01T00:00:00+00:00",
                         "last_used_at": "2026-10-11T12:00:00+00:00"})),
        ]})))
        .mount(&server)
        .await;
    let list = client.list_tokens("prj_a").await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].expires_at, None);
    assert_eq!(list[1].name, "deploy");
    assert_eq!(
        list[1].last_used_at.as_deref(),
        Some("2026-10-11T12:00:00+00:00")
    );
    // The list never has the secret.
    assert!(!serde_json::to_string(&list).unwrap().contains("token\""));
}

#[tokio::test]
async fn create_sends_null_for_never_and_returns_the_secret() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/tokens"))
        .and(body_json(json!({"name": "ci", "expires_in_days": null})))
        .respond_with(ResponseTemplate::new(201).set_body_json(token(json!({"token": TOKEN}))))
        .expect(1)
        .mount(&server)
        .await;
    let created: CreatedApiToken = client.create_token("prj_a", "ci", None).await.unwrap();
    assert_eq!(created.token, TOKEN);
    assert_eq!(created.expires_at, None);
}

#[tokio::test]
async fn create_sends_days() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/tokens"))
        .and(body_json(json!({"name": "ci", "expires_in_days": 90})))
        .respond_with(ResponseTemplate::new(201).set_body_json(token(json!({
            "token": TOKEN, "expires_at": "2027-01-09T10:00:00+00:00"}))))
        .expect(1)
        .mount(&server)
        .await;
    let created = client.create_token("prj_a", "ci", Some(90)).await.unwrap();
    assert_eq!(
        created.expires_at.as_deref(),
        Some("2027-01-09T10:00:00+00:00")
    );
}

#[tokio::test]
async fn revoke_deletes_the_token() {
    let (server, client) = common::setup().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/tokens/tok_abc123"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    client.revoke_token("prj_a", "tok_abc123").await.unwrap();
}

#[tokio::test]
async fn refused_token_route_shows_the_api_message() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/tokens"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": "not_allowed", "message": "This needs a sign-in. An API token cannot use this route."}})))
        .mount(&server)
        .await;
    let err = client.list_tokens("prj_a").await.unwrap_err();
    assert_eq!(err.status(), Some(403));
    assert_eq!(
        err.to_string(),
        "This needs a sign-in. An API token cannot use this route."
    );
}

#[tokio::test]
async fn a_static_token_is_not_refreshed_on_401() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {
            "code": "unauthorized", "message": "The API token is not valid."}})))
        .expect(1)
        .mount(&server)
        .await;
    let err = client.me().await.unwrap_err();
    assert_eq!(err.to_string(), "The API token is not valid.");
}
