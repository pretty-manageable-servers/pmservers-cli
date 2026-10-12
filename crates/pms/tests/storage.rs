mod common;

use common::{project_json_owned, Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OWNER: &str = "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90";
const KEY: &str = "GK0123456789abcdef01234567";
const SECRET: &str = "s3cr3t-value-shown-once";

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

fn storage_json(status: &str) -> serde_json::Value {
    json!({"status": status, "status_message": null, "endpoint": "https://s3.pmservers.org",
           "region": "pms", "used_bytes": 1073741824u64, "limit_bytes": 10737418240u64,
           "buckets_count": 1, "buckets_limit": 10, "keys_count": 2, "keys_limit": 20,
           "would_cost_usd": 0.0})
}

fn bucket_json(name: &str, public: bool) -> serde_json::Value {
    json!({"name": name, "public_read": public, "status": "active", "status_message": null,
           "used_bytes": 2048, "object_count": 3, "created_at": "2026-10-11T10:00:00+00:00",
           "updated_at": "2026-10-11T10:00:00+00:00"})
}

fn key_json(buckets: serde_json::Value, permission: &str) -> serde_json::Value {
    json!({"key_id": KEY, "name": "ci", "permission": permission, "buckets": buckets,
           "created_at": "2026-10-11T10:00:00+00:00", "updated_at": "2026-10-11T10:00:00+00:00"})
}

fn secret_json(buckets: serde_json::Value, permission: &str) -> serde_json::Value {
    let mut v = key_json(buckets, permission);
    v.as_object_mut().unwrap().extend(
        json!({"endpoint": "https://s3.pmservers.org", "region": "pms",
               "secret_access_key": SECRET})
        .as_object()
        .unwrap()
        .clone(),
    );
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn status_shows_usage_and_limits() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(storage_json("active")))
        .expect(2)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "status"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("active")
                .and(predicate::str::contains("1.00 GiB used of 10.00 GiB"))
                .and(predicate::str::contains("not charged yet")),
        );
    let out = env
        .cmd(&api.uri())
        .args(["-o", "json", "storage", "status"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["buckets_limit"], 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn api_tokens_are_refused_before_any_call() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .respond_with(ResponseTemplate::new(403))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .env("PMS_TOKEN", "pms_abcd1234")
        .args(["storage", "status"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("does not accept API tokens")
                .and(predicate::str::contains("pms login")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn api_refusal_shows_the_api_message() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/enable"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "not_allowed",
            "message": "Object storage is for approved accounts. Ask the PMServers team."}})),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "enable"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("for approved accounts"));
}

#[tokio::test(flavor = "multi_thread")]
async fn account_flag_sends_the_owner_id() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .and(query_param("account_id", OWNER))
        .respond_with(ResponseTemplate::new(200).set_body_json(storage_json("active")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "status", "--account", OWNER])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn project_flag_uses_the_owner_of_the_project() {
    let (api, env) = setup().await;
    const OTHER_OWNER: &str = "0c9e1f7a-2b3d-4e5f-8a6b-7c8d9e0f1a2b";
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json_owned(
            PRJ,
            "slop",
            "member",
            OTHER_OWNER,
        )))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .and(query_param("account_id", OTHER_OWNER))
        .respond_with(ResponseTemplate::new(200).set_body_json(storage_json("active")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "status", "--project", PRJ])
        .assert()
        .success()
        .stdout(predicate::str::contains("active"));
}

#[tokio::test(flavor = "multi_thread")]
async fn account_and_project_conflict() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(storage_json("active")))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "status", "--account", OWNER, "--project", PRJ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used together"));
    env.cmd(&api.uri())
        .args(["--project", PRJ, "storage", "status", "--account", OWNER])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used together"));
}

#[tokio::test(flavor = "multi_thread")]
async fn account_flag_works_with_project_env_set() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage"))
        .and(query_param("account_id", OWNER))
        .respond_with(ResponseTemplate::new(200).set_body_json(storage_json("active")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .env("PMS_PROJECT", PRJ)
        .args(["storage", "status", "--account", OWNER])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn not_available_has_a_clear_message() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/enable"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code":
            "not_available", "message": "Object storage is not available now."}})),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "enable"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("switched off for all accounts")
                .and(predicate::str::contains("Try again later")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn enable_and_disable() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/enable"))
        .respond_with(ResponseTemplate::new(202).set_body_json(storage_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/disable"))
        .respond_with(ResponseTemplate::new(202).set_body_json(storage_json("disabling")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "enable"])
        .assert()
        .success()
        .stdout(predicate::str::contains("provisioning"));
    env.cmd(&api.uri())
        .args(["storage", "disable"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-y", "storage", "disable"])
        .assert()
        .success()
        .stdout(predicate::str::contains("disabling"));
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_list_shows_the_buckets() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage/buckets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            bucket_json("photos", true), bucket_json("logs", false)]})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "bucket", "ls"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("photos")
                .and(predicate::str::contains("logs"))
                .and(predicate::str::contains("2.0 KiB")),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_create_with_public_read() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/buckets"))
        .and(body_json(json!({"name": "photos", "public_read": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(bucket_json("photos", true)))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "bucket", "create", "photos", "--public"])
        .assert()
        .success()
        .stdout(predicate::str::contains("photos").and(predicate::str::contains("Public read")));
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_create_reports_a_taken_name() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/buckets"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({"error": {"code": "conflict",
            "message": "That bucket name is taken."}})),
        )
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "bucket", "create", "photos"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("That bucket name is taken."));
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_rm_asks_first_then_deletes() {
    let (api, env) = setup().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/storage/buckets/photos"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "bucket", "rm", "photos"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-y", "storage", "bucket", "rm", "photos"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleting bucket photos."));
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_public_on_and_off() {
    let (api, env) = setup().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/storage/buckets/photos"))
        .and(body_json(json!({"public_read": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(bucket_json("photos", true)))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1/storage/buckets/photos"))
        .and(body_json(json!({"public_read": false})))
        .respond_with(ResponseTemplate::new(202).set_body_json(bucket_json("photos", false)))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "bucket", "public", "photos", "on"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Public read:").and(predicate::str::contains("yes")));
    env.cmd(&api.uri())
        .args(["storage", "bucket", "public", "photos", "off"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn key_create_defaults_to_all_buckets_and_read_write() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/keys"))
        .and(body_json(
            json!({"name": "ci", "permission": "read_write", "buckets": "all"}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(secret_json(json!("all"), "read_write")),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "key", "create", "ci"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(KEY)
                .and(predicate::str::contains(SECRET))
                .and(predicate::str::contains("https://s3.pmservers.org"))
                .and(predicate::str::contains("pms")),
        )
        .stderr(predicate::str::contains("shown one time"));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_create_read_only_for_chosen_buckets() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/keys"))
        .and(body_json(
            json!({"name": "ci", "permission": "read_only", "buckets": ["a", "b"]}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(secret_json(json!(["a", "b"]), "read_only")),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "storage",
            "key",
            "create",
            "ci",
            "--read-only",
            "--bucket",
            "a",
            "--bucket",
            "b",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("a, b").and(predicate::str::contains("read_only")));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_create_env_prints_only_export_lines() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/keys"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(secret_json(json!("all"), "read_write")),
        )
        .expect(1)
        .mount(&api)
        .await;
    let out = env
        .cmd(&api.uri())
        .args(["storage", "key", "create", "ci", "--env"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with(
        "# Use path style: aws configure set default.s3.addressing_style path\nexport AWS_ACCESS_KEY_ID='"
    ));
    assert!(stdout.contains(&format!("export AWS_SECRET_ACCESS_KEY='{SECRET}'\n")));
    assert!(stdout.contains("export AWS_ENDPOINT_URL='https://s3.pmservers.org'\n"));
    assert!(stdout.contains("export AWS_DEFAULT_REGION='pms'\n"));
    assert!(!stdout.contains("Secret access key"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("shown one time"));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_create_env_with_json_fails_before_the_call() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/storage/keys"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(secret_json(json!("all"), "read_write")),
        )
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-o", "json", "storage", "key", "create", "ci", "--env"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--env cannot be used with --output json",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_reset_prints_the_new_secret() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/storage/keys/{KEY}/reset")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(secret_json(json!("all"), "read_write")),
        )
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "key", "reset", KEY])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-y", "storage", "key", "reset", KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains(SECRET))
        .stderr(predicate::str::contains("shown one time"));
}

#[tokio::test(flavor = "multi_thread")]
async fn key_ls_and_rm() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/storage/keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            key_json(json!("all"), "read_write")]})))
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/storage/keys/{KEY}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["storage", "key", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains(KEY).and(predicate::str::contains("read_write")))
        .stdout(predicate::str::contains(SECRET).not());
    env.cmd(&api.uri())
        .args(["-y", "storage", "key", "rm", KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("Deleted key {KEY}.")));
}
