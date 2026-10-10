mod common;

use common::{read, Env};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn me_body() -> serde_json::Value {
    json!({"id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90", "email": "a@b.co", "plan": "free"})
}

#[test]
fn help_lists_commands() {
    let env = Env::new();
    env.cmd("http://unused")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("login").and(predicate::str::contains("whoami")));
}

#[test]
fn version_prints() {
    Env::new()
        .cmd("http://unused")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("pms "));
}

#[test]
fn unknown_command_fails() {
    Env::new()
        .cmd("http://unused")
        .arg("nope")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn whoami_without_login_says_how_to_sign_in() {
    Env::new()
        .cmd("http://unused")
        .arg("whoami")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("pms login"));
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_prints_table_and_json() {
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body()))
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd(&api.uri())
        .arg("whoami")
        .assert()
        .success()
        .stdout(predicate::str::contains("a@b.co").and(predicate::str::contains("free")));
    let out = env
        .cmd(&api.uri())
        .args(["-o", "json", "whoami"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["email"], "a@b.co");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_error_message_and_exit_code() {
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": {"code": "not_allowed", "message": "Nope."}})),
        )
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd(&api.uri())
        .arg("whoami")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("error: Nope."));
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_session_is_refreshed_and_saved() {
    let auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(header("apikey", "anon"))
        .and(body_json(json!({"refresh_token": "refresh-1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600,
            "token_type": "bearer",
            "user": {"id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90", "email": "a@b.co",
                     "app_metadata": {"provider": "github"}}
        })))
        .expect(1)
        .mount(&auth)
        .await;
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer access-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body()))
        .expect(1)
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in(&auth.uri(), -10);
    env.cmd(&api.uri())
        .env("PMS_SUPABASE_ANON_KEY", "anon")
        .env("PMS_SUPABASE_URL", "https://other.example")
        .arg("whoami")
        .assert()
        .success();
    let saved = read(&env.credentials());
    assert_eq!(saved["refresh_token"], "refresh-2");
    assert_eq!(saved["provider"], "github");
    // The session keeps its own Supabase project.
    assert_eq!(saved["supabase_url"], auth.uri());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(env.credentials())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_401_triggers_one_refresh_and_a_retry() {
    let auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600,
            "user": {"id": "u", "email": "a@b.co", "app_metadata": {}}
        })))
        .expect(1)
        .mount(&auth)
        .await;
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(
                json!({"error": {"code": "unauthorized", "message": "Sign in again."}}),
            ),
        )
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer access-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body()))
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in(&auth.uri(), 3600);
    env.cmd(&api.uri())
        .env("PMS_SUPABASE_ANON_KEY", "anon")
        .arg("whoami")
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn logout_revokes_locally_scoped_and_removes_the_session() {
    let auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/logout"))
        .and(query_param("scope", "local"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&auth)
        .await;
    let env = Env::new();
    env.sign_in(&auth.uri(), 3600);
    env.cmd("http://unused")
        .env("PMS_SUPABASE_URL", "https://ignored.example")
        .env("PMS_SUPABASE_ANON_KEY", "anon")
        .arg("logout")
        .assert()
        .success()
        .stdout(predicate::str::contains("Signed out"));
    assert!(!env.credentials().exists());
}

#[test]
fn logout_removes_the_session_even_without_the_anon_key() {
    if option_env!("PMS_SUPABASE_URL").is_some() {
        return;
    }
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd("http://unused")
        .arg("logout")
        .assert()
        .success()
        .stderr(predicate::str::contains("could not be revoked"));
    assert!(!env.credentials().exists());
}

#[test]
fn login_without_supabase_settings_names_the_variables() {
    if option_env!("PMS_SUPABASE_URL").is_some() {
        return;
    }
    Env::new()
        .cmd("http://unused")
        .arg("login")
        .assert()
        .failure()
        .stderr(predicate::str::contains("PMS_SUPABASE_URL"));
}

#[test]
fn login_email_needs_a_terminal() {
    Env::new()
        .cmd("http://unused")
        .env("PMS_SUPABASE_URL", "https://x.example")
        .env("PMS_SUPABASE_ANON_KEY", "anon")
        .args(["login", "--email"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs a terminal"));
}

#[test]
fn login_flags_conflict() {
    Env::new()
        .cmd("http://unused")
        .args(["login", "--email", "--port", "5000"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn catalog_json_and_table() {
    let api = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/catalog"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "regions": ["ca-tor-1"],
            "vm_sizes": [{"id": "pms-s-1", "vcpu": 1, "memory_gib": 2}],
            "container_sizes": [{"id": "pms-c-250m-512", "vcpu": 0.25, "memory_mib": 512}],
            "database_sizes": [{"id": "pms-db-250m-512", "vcpu": 0.25, "memory_mib": 512}],
            "vm_images": [{"id": "ubuntu-24.04", "name": "Ubuntu", "version": "24.04"}],
            "ai_models": [{"id": "gemma4-e4b", "type": "llm", "description": "Small model", "detail": ""}]
        })))
        .mount(&api)
        .await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd(&api.uri())
        .args(["catalog", "models"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("gemma4-e4b").and(predicate::str::contains("pms-s-1").not()),
        );
    env.cmd(&api.uri())
        .args(["catalog", "sizes"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pms-s-1").and(predicate::str::contains("pms-c-250m-512")),
        );
    let out = env
        .cmd(&api.uri())
        .args(["-o", "json", "catalog"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["vm_images"][0]["id"], "ubuntu-24.04");
}
