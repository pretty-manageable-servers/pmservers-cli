mod common;

use common::{container_json, Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CTR: &str = "ctr_abc123def456";

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

async fn mount_list(api: &MockServer, status: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"items": [container_json(status)]})),
        )
        .mount(api)
        .await;
}

fn access(state: &str) -> serde_json::Value {
    json!({"access": state, "runtime": "gvisor", "vcluster": "none", "reason": null, "note": null,
           "requested_at": null, "decided_at": null})
}

#[tokio::test(flavor = "multi_thread")]
async fn access_show_and_request() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/account/access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(access("none")))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/account/access/request"))
        .and(body_json(json!({"reason": "Batch jobs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(access("pending")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["access", "show"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("none").and(predicate::str::contains("pms access request")),
        );
    env.cmd(&api.uri())
        .args(["access", "request", "Batch jobs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("waiting for review"));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_show_and_the_approval_error() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(container_json("running")))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "container", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(CTR).and(predicate::str::contains("ghcr.io/o/app:1")));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "container", "show", "api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Variables:").and(predicate::str::contains("A, B")));
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": "not_allowed", "message": "Containers need approval. Request access on the Account page."}})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "container", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Containers need approval"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_with_env_file_credential_by_name_and_wait() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/registry-credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "reg_1", "name": "ghcr", "host": "ghcr.io", "username": "bot",
             "created_at": "2026-10-10T10:00:00+00:00"}]})))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/containers")))
        .and(body_json(
            json!({"name": "api", "type": "worker", "image": "ghcr.io/o/app:1",
                              "size": "pms-c-250m-512", "registry_credential_id": "reg_1",
                              "env": {"A": "file", "B": "flag"}}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(container_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(container_json("running")))
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
    let file = env.dir.path().join(".env");
    std::fs::write(&file, "A=file\n# comment\n").unwrap();
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "container",
            "create",
            "api",
            "--image",
            "ghcr.io/o/app:1",
            "--size",
            "pms-c-250m-512",
        ])
        .args([
            "--registry-credential",
            "ghcr",
            "-e",
            "B=flag",
            "--env-file",
        ])
        .arg(&file)
        .arg("--wait")
        .assert()
        .success()
        .stdout(predicate::str::contains("running"));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_sends_only_what_changed_and_says_env_replaces_all() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .and(body_json(json!({"image": "ghcr.io/o/app:2"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(container_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .and(body_json(
            json!({"registry_credential_id": null, "env": {}}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(container_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .and(body_json(json!({"env": {"X": "1"}})))
        .respond_with(ResponseTemplate::new(202).set_body_json(container_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    let base = ["-p", PRJ, "container", "edit", "api"];
    env.cmd(&api.uri())
        .args(base)
        .args(["--image", "ghcr.io/o/app:2"])
        .assert()
        .success();
    env.cmd(&api.uri())
        .args(base)
        .args(["--no-registry-credential", "--clear-env"])
        .assert()
        .success();
    env.cmd(&api.uri())
        .args(base)
        .args(["-e", "X=1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("replace all variables"));
    env.cmd(&api.uri())
        .args(base)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Nothing to change"));
    env.cmd(&api.uri())
        .args(base)
        .args(["--clear-env", "-e", "X=1"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn actions_and_delete() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}/actions")))
        .respond_with(ResponseTemplate::new(202).set_body_json(container_json("stopped")))
        .expect(3)
        .mount(&api)
        .await;
    for a in ["start", "stop", "restart"] {
        env.cmd(&api.uri())
            .args(["-p", PRJ, "container", a, "api"])
            .assert()
            .success();
    }
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}")))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "container", "delete", "api"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "container", "delete", "api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleting container api"));
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_tail_and_previous() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}/logs")))
        .and(query_param("tail", "5"))
        .and(query_param("previous", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"lines": [
            {"time": "2026-10-10T10:00:00.1Z", "message": "boom"}]})))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "container",
            "logs",
            "api",
            "--tail",
            "5",
            "--previous",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("2026-10-10T10:00:00.1Z boom"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "container", "logs", "api", "--tail", "5000"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_follow_prints_only_new_lines() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    let first = json!({"lines": [{"time": "1", "message": "one"}]});
    let second =
        json!({"lines": [{"time": "1", "message": "one"}, {"time": "2", "message": "two"}]});
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}/logs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .up_to_n_times(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/containers/{CTR}/logs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(second))
        .mount(&api)
        .await;
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("pms"))
        .env("PMS_CONFIG_DIR", env.config_dir())
        .env("PMS_NO_KEYRING", "1")
        .args([
            "--api-url",
            &api.uri(),
            "-p",
            PRJ,
            "container",
            "logs",
            "api",
            "--follow",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let (mut a, mut b) = (String::new(), String::new());
    reader.read_line(&mut a).unwrap();
    reader.read_line(&mut b).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(a.trim(), "1 one");
    assert_eq!(b.trim(), "2 two");
}

#[tokio::test(flavor = "multi_thread")]
async fn registry_credentials_token_from_stdin_and_never_in_output() {
    let (api, env) = setup().await;
    let cred = json!({"id": "reg_1", "name": "ghcr", "host": "ghcr.io", "username": "bot",
                      "created_at": "2026-10-10T10:00:00+00:00"});
    Mock::given(method("POST"))
        .and(path("/v1/registry-credentials"))
        .and(body_json(
            json!({"name": "ghcr", "host": "ghcr.io", "username": "bot", "token": "s3cret"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(cred.clone()))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/registry-credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [cred]})))
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/registry-credentials/reg_1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    let out = env
        .cmd(&api.uri())
        .args([
            "registry-credential",
            "add",
            "ghcr",
            "--host",
            "ghcr.io",
            "--username",
            "bot",
            "--token-stdin",
        ])
        .write_stdin("s3cret\n")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("s3cret"));
    env.cmd(&api.uri())
        .args([
            "registry-credential",
            "add",
            "ghcr",
            "--host",
            "ghcr.io",
            "--username",
            "bot",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--token-stdin"));
    env.cmd(&api.uri())
        .args(["registry-credential", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghcr.io"));
    env.cmd(&api.uri())
        .args(["-y", "registry-credential", "delete", "ghcr"])
        .assert()
        .success();
}
