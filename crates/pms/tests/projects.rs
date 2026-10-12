mod common;

use common::{project_json, Env};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

async fn mount_projects(api: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            project_json("prj_aaaaaaaaaaaa", "web", "owner"),
            project_json("prj_bbbbbbbbbbbb", "shared", "member"),
        ]})))
        .mount(api)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn project_list_table_and_json() {
    let (api, env) = setup().await;
    mount_projects(&api).await;
    env.cmd(&api.uri())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("ID")
                .and(predicate::str::contains("prj_aaaaaaaaaaaa"))
                .and(predicate::str::contains("shared")),
        );
    let out = env
        .cmd(&api.uri())
        .args(["-o", "json", "project", "list"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[1]["role"], "member");
}

#[tokio::test(flavor = "multi_thread")]
async fn project_use_by_name_saves_the_default() {
    let (api, env) = setup().await;
    mount_projects(&api).await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_bbbbbbbbbbbb"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json(
            "prj_bbbbbbbbbbbb",
            "shared",
            "member",
        )))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["project", "use", "shared"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prj_bbbbbbbbbbbb"));
    let config = std::fs::read_to_string(env.config_dir().join("config.toml")).unwrap();
    assert!(config.contains("prj_bbbbbbbbbbbb"));
    // The saved default is used by the next command.
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_bbbbbbbbbbbb/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "vcpu": {"used": 0, "limit": 2, "series": []},
            "memory_gib": {"used": 0, "limit": 4, "series": []},
            "ai_spend_usd": {"used": 0.5, "limit": 5.0, "series": []}
        })))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["project", "usage"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AI spend").and(predicate::str::contains("$0.50")));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_project_name_is_an_error() {
    let (api, env) = setup().await;
    mount_projects(&api).await;
    env.cmd(&api.uri())
        .args(["--project", "nope", "project", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No project named"));
}

#[test]
fn no_project_set_says_how_to_set_one() {
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    env.cmd("http://unused")
        .args(["project", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("pms project use"));
}

#[tokio::test(flavor = "multi_thread")]
async fn project_create_with_use() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects"))
        .and(body_json(json!({"name": "fresh"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(project_json(
            "prj_cccccccccccc",
            "fresh",
            "owner",
        )))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["project", "create", "fresh", "--use"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fresh"));
    assert!(
        std::fs::read_to_string(env.config_dir().join("config.toml"))
            .unwrap()
            .contains("prj_cccccccccccc")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn project_delete_needs_yes_without_a_terminal() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json(
            "prj_aaaaaaaaaaaa",
            "web",
            "owner",
        )))
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["--project", "prj_aaaaaaaaaaaa", "project", "delete"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args([
            "--project",
            "prj_aaaaaaaaaaaa",
            "--yes",
            "project",
            "delete",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleting project web"));
}

#[tokio::test(flavor = "multi_thread")]
async fn project_delete_conflict_shows_the_api_message() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json(
            "prj_aaaaaaaaaaaa",
            "web",
            "owner",
        )))
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({"error": {"code": "conflict",
            "message": "Delete the containers, databases and AI keys of this project first."}})),
        )
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", "prj_aaaaaaaaaaaa", "-y", "project", "delete"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Delete the containers"));
}

#[tokio::test(flavor = "multi_thread")]
async fn leave_removes_yourself() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_bbbbbbbbbbbb"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_json(
            "prj_bbbbbbbbbbbb",
            "shared",
            "member",
        )))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "uid-1", "email": "a@b.co", "plan": "free"})),
        )
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_bbbbbbbbbbbb/members/uid-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", "prj_bbbbbbbbbbbb", "-y", "project", "leave"])
        .assert()
        .success()
        .stdout(predicate::str::contains("You left"));
}

#[tokio::test(flavor = "multi_thread")]
async fn members_invite_and_remove_by_email() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"user_id": "u1", "email": "o@x.co", "role": "owner", "created_at": "2026-10-09T10:00:00+00:00"},
            {"user_id": "u2", "email": "m@x.co", "role": "member", "created_at": "2026-10-09T11:00:00+00:00"}
        ]})))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa/members"))
        .and(body_json(json!({"email": "n@x.co"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "user_id": "u3", "email": "n@x.co", "role": "member",
            "created_at": "2026-10-10T10:00:00+00:00", "invited": true})))
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_aaaaaaaaaaaa/members/u2"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    let base = ["-p", "prj_aaaaaaaaaaaa"];
    env.cmd(&api.uri())
        .args(base)
        .args(["member", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("m@x.co"));
    env.cmd(&api.uri())
        .args(base)
        .args(["member", "invite", "n@x.co"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Invited n@x.co"));
    env.cmd(&api.uri())
        .args(base)
        .args(["-y", "member", "remove", "M@X.co"])
        .assert()
        .success();
    env.cmd(&api.uri())
        .args(base)
        .args(["-y", "member", "remove", "ghost@x.co"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No member"));
}
