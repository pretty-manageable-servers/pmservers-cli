mod common;

use common::{project_json, Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "pms_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";

fn token_json(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({"id": "tok_abc123def456", "name": "ci", "prefix": "pms_0123",
                       "created_at": "2026-10-11T10:00:00+00:00", "expires_at": null,
                       "last_used_at": null});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

fn me_body() -> serde_json::Value {
    json!({"id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90", "email": "a@b.co", "plan": "free"})
}

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

#[tokio::test(flavor = "multi_thread")]
async fn create_shows_the_token_once_with_a_warning() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .and(body_json(json!({"name": "ci", "expires_in_days": null})))
        .respond_with(ResponseTemplate::new(201).set_body_json(token_json(json!({"token": TOKEN}))))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "token", "create", "--name", "ci"])
        .assert()
        .success()
        .stdout(predicate::str::contains(TOKEN).and(predicate::str::contains("never")))
        .stderr(predicate::str::contains("shown only once"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_with_expiry_in_days() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .and(body_json(json!({"name": "ci", "expires_in_days": 90})))
        .respond_with(ResponseTemplate::new(201).set_body_json(token_json(json!({
            "token": TOKEN, "expires_at": "2027-01-09T10:00:00+00:00"}))))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "token",
            "create",
            "--name",
            "ci",
            "--expires",
            "90d",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("2027-01-09 10:00:00"));
}

#[test]
fn expiry_must_be_one_to_365_days_or_never() {
    let env = Env::new();
    for bad in ["0d", "366d", "90", "abc"] {
        env.cmd("http://unused")
            .args([
                "-p",
                PRJ,
                "token",
                "create",
                "--name",
                "ci",
                "--expires",
                bad,
            ])
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("Use 1d to 365d, or never."));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn create_json_prints_the_full_response() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .respond_with(ResponseTemplate::new(201).set_body_json(token_json(json!({"token": TOKEN}))))
        .mount(&api)
        .await;
    let out = env
        .cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "-o",
            "json",
            "token",
            "create",
            "--name",
            "ci",
            "--expires",
            "never",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("shown only once"));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["token"], TOKEN);
    assert_eq!(v["id"], "tok_abc123def456");
    assert!(v["expires_at"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_shows_id_name_prefix_expiry_last_used_and_created() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            token_json(json!({})),
            token_json(json!({"id": "tok_def456", "name": "deploy", "prefix": "pms_9876",
                              "expires_at": "2027-01-01T00:00:00+00:00",
                              "last_used_at": "2026-10-11T12:30:00+00:00"})),
        ]})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "token", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("tok_abc123def456")
                .and(predicate::str::contains("pms_0123"))
                .and(predicate::str::contains("never"))
                .and(predicate::str::contains("2026-10-11 12:30:00"))
                .and(predicate::str::contains("2027-01-01 00:00:00"))
                .and(predicate::str::contains(TOKEN).not()),
        );
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "-o", "json", "token", "list"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[1]["name"], "deploy");
}

#[tokio::test(flavor = "multi_thread")]
async fn revoke_with_yes_deletes_the_token() {
    let (api, env) = setup().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/projects/{PRJ}/tokens/tok_abc123def456")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "--yes", "token", "revoke", "tok_abc123def456"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Revoked API token tok_abc123def456.",
        ));
}

#[test]
fn revoke_without_a_terminal_needs_yes() {
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd("http://unused")
        .args(["-p", PRJ, "token", "revoke", "tok_abc123def456"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Pass --yes"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pms_token_wins_over_the_saved_session_and_skips_refresh() {
    let auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&auth)
        .await;
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body()))
        .expect(1)
        .mount(&api)
        .await;
    let env = Env::new();
    // An expired saved session. It must not be used or refreshed.
    env.sign_in(&auth.uri(), -10);
    env.cmd(&api.uri())
        .env("PMS_TOKEN", TOKEN)
        .arg("whoami")
        .assert()
        .success()
        .stdout(predicate::str::contains("a@b.co"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pms_token_works_without_a_saved_session() {
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            project_json(PRJ, "slop", "member")]})))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
        .expect(1)
        .mount(&api)
        .await;
    let env = Env::new();
    // Use the project name. The CLI finds the ID through the bound project list.
    env.cmd(&api.uri())
        .env("PMS_TOKEN", TOKEN)
        .args(["-p", "slop", "token", "list"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_is_not_refreshed_and_shows_the_api_message() {
    let auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&auth)
        .await;
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {
            "code": "unauthorized", "message": "The API token is not valid."}})))
        .expect(1)
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in(&auth.uri(), -10);
    env.cmd(&api.uri())
        .env("PMS_TOKEN", "pms_wrong")
        .arg("whoami")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "error: The API token is not valid.",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn token_commands_refused_for_tokens_show_the_api_403_message() {
    let api = MockServer::start().await;
    let message = "This needs a sign-in. An API token cannot use this route.";
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/tokens")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": "not_allowed", "message": message}})))
        .mount(&api)
        .await;
    let env = Env::new();
    env.cmd(&api.uri())
        .env("PMS_TOKEN", TOKEN)
        .args(["-p", PRJ, "token", "list"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!("error: {message}")));
}
