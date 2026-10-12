mod common;

use common::{Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DB: &str = "db_abc123def456";

fn db(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({
        "id": DB, "name": "main", "engine": "postgres", "size": "pms-db-250m-512", "vcpu": 0.25,
        "memory_mib": 512, "vector": false, "status": "running", "status_message": null,
        "host": "db.internal", "port": 5432, "database": "app", "user": "app",
        "used_bytes": 1048576, "storage_limit_bytes": 1073741824,
        "created_at": "2026-10-10T10:00:00+00:00", "updated_at": "2026-10-10T10:00:00+00:00"});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

async fn mount_list(api: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/databases")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [db(json!({}))]})))
        .mount(api)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn create_shows_the_password_once_with_a_warning() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/databases")))
        .and(body_json(json!({"name": "main", "vector": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(db(json!({
            "status": "provisioning", "password": "pw-123",
            "connection_url": "postgresql://app:pw-123@db.internal:5432/app"}))))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "create", "main", "--vector"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pw-123")
                .and(predicate::str::contains("postgresql://app:pw-123@")),
        )
        .stderr(predicate::str::contains("shown one time"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_wait_prints_the_password_before_waiting() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/databases")))
        .respond_with(ResponseTemplate::new(202).set_body_json(db(json!({
            "status": "provisioning", "password": "pw-123", "connection_url": "u"}))))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/databases/{DB}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(db(json!({}))))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/events/ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticket": "t"})))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "create", "main", "--wait"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pw-123")
                .and(predicate::str::contains("Database main is running.")),
        );
    // JSON mode: one object at the end, with the secret from the create call.
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "-o", "json", "db", "create", "main", "--wait"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["password"], "pw-123");
    assert_eq!(v["status"], "running");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_and_show_never_have_a_password() {
    let (api, env) = setup().await;
    mount_list(&api).await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/databases/{DB}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(db(json!({}))))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(DB).and(predicate::str::contains("1.0 MiB / 1024.0 MiB")));
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "db", "show", "main"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("db.internal") && !text.contains("Password"));
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "-o", "json", "db", "show", "main"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("password"));
}

#[tokio::test(flavor = "multi_thread")]
async fn vector_on_off_and_the_conflict() {
    let (api, env) = setup().await;
    mount_list(&api).await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/projects/{PRJ}/databases/{DB}")))
        .and(body_json(json!({"vector": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(db(json!({"vector": true}))))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/projects/{PRJ}/databases/{DB}")))
        .and(body_json(json!({"vector": false})))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({"error": {"code": "conflict",
            "message": "A column or index uses vector. Remove it first."}})),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "vector", "on", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Vector:").and(predicate::str::contains("on")));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "vector", "off", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("A column or index uses vector"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_password_confirms_and_prints_the_new_one() {
    let (api, env) = setup().await;
    mount_list(&api).await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1/projects/{PRJ}/databases/{DB}/reset-password"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(db(json!({
            "status": "provisioning", "password": "new-pw", "connection_url": "u"}))))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "reset-password", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "db", "reset-password", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("new-pw"));
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_warns_about_no_backups() {
    let (api, env) = setup().await;
    mount_list(&api).await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/projects/{PRJ}/databases/{DB}")))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "db", "delete", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no backups"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "db", "delete", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleting database main"));
}

fn key(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({"id": "key_abc123def456", "name": "app", "status": "active", "prefix": "pms_sk_ab",
        "models": ["gemma4-e4b"], "budget_usd": 5.0, "spend_usd": 0.25, "rate_limit_rpm": 60,
        "created_at": "2026-10-10T10:00:00+00:00"});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_key_create_shows_the_secret_once() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/ai-keys")))
        .and(body_json(
            json!({"name": "app", "models": ["gemma4-e4b", "kev"], "budget_usd": 5.0}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(key(json!({"secret": "pms_sk_full_secret"}))),
        )
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/ai-keys")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [key(json!({}))]})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "ai-key",
            "create",
            "app",
            "--model",
            "gemma4-e4b",
            "--model",
            "kev",
            "--budget",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("pms_sk_full_secret"))
        .stderr(predicate::str::contains("shown one time"));
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "ai-key", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("pms_sk_ab") && text.contains("$0.25 / $5.00"));
    assert!(!text.contains("secret"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "ai-key", "create", "app", "--budget", "5"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_key_edit_rotate_delete() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/ai-keys")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [key(json!({}))]})))
        .mount(&api)
        .await;
    let key_path = format!("/v1/projects/{PRJ}/ai-keys/key_abc123def456");
    Mock::given(method("PATCH"))
        .and(path(&key_path))
        .and(body_json(json!({"models": ["kev"], "budget_usd": 10.0})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(key(json!({"models": ["kev"], "budget_usd": 10.0}))),
        )
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{key_path}/rotate")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(key(json!({"secret": "pms_sk_rotated"}))),
        )
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path(&key_path))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p", PRJ, "ai-key", "edit", "app", "--model", "kev", "--budget", "10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("$10.00"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "ai-key", "edit", "app"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Nothing to change"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "ai-key", "rotate", "app"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "ai-key", "rotate", "app"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pms_sk_rotated"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "ai-key", "delete", "app"])
        .assert()
        .success();
}

#[test]
fn db_connect_is_removed() {
    // The jump host is gone. `pms network` replaces it.
    Env::new()
        .cmd("http://unused")
        .args(["db", "connect", "main"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("connect"));
}
