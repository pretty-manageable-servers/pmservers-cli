mod common;

use common::{Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CTR: &str = "ctr_abc123def456";
const DB: &str = "db_abc123def456";

fn target(kind: &str, id: &str, name: &str, port: u16, external: Option<u16>) -> serde_json::Value {
    json!({"kind": kind, "id": id, "name": name, "port": port, "external_port": external,
           "address": external.map(|p| format!("prj-a.tn.ts.net:{p}"))})
}

fn connector(kind: &str, status: &str, targets: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"id": format!("ncn_{kind}"), "kind": kind, "status": status, "message": null,
           "observed_at": null, "targets": targets,
           "created_at": "2026-10-11T10:00:00+00:00", "updated_at": "2026-10-11T10:00:00+00:00"})
}

fn available() -> serde_json::Value {
    json!({"targets": [
        {"kind": "container", "id": CTR, "name": "api", "ports": [3000]},
        {"kind": "database", "id": DB, "name": "main", "ports": [5432]}]})
}

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

async fn mount_connectors(api: &MockServer, list: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/connectors")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"connectors": list})))
        .mount(api)
        .await;
}

async fn mount_available(api: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/projects/{PRJ}/connectors/available-targets"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(available()))
        .mount(api)
        .await;
}

async fn mount_put(api: &MockServer, kind: &str, body: serde_json::Value, times: u64) {
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/{kind}")))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector(
            kind,
            "connected",
            vec![],
        )))
        .expect(times)
        .mount(api)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn show_lists_connectors_status_and_addresses() {
    let (api, env) = setup().await;
    mount_connectors(
        &api,
        vec![connector(
            "tailscale",
            "connected",
            vec![target("container", CTR, "api", 3000, Some(3000))],
        )],
    )
    .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "show"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("tailscale")
                .and(predicate::str::contains("connected"))
                .and(predicate::str::contains("cloudflared"))
                .and(predicate::str::contains("off"))
                .and(predicate::str::contains("prj-a.tn.ts.net:3000")),
        );
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "-o", "json", "network", "show"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["kind"], "tailscale");
    assert_eq!(v[0]["targets"][0]["address"], "prj-a.tn.ts.net:3000");
}

#[cfg(unix)]
fn private_file(env: &Env, name: &str, text: &str, mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = env.dir.path().join(name);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn tailscale_enable_reads_the_key_from_a_private_file_and_never_prints_it() {
    let (api, env) = setup().await;
    let key = private_file(&env, "key", "tskey-auth-TEST\n", 0o600);
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/tailscale")))
        .and(body_json(json!({"token": "tskey-auth-TEST"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector(
            "tailscale",
            "provisioning",
            vec![],
        )))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "network",
            "tailscale",
            "enable",
            "--auth-key-file",
        ])
        .arg(&key)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("provisioning")
                .and(predicate::str::contains("tskey-auth-TEST").not()),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn cloudflared_enable_reads_standard_input() {
    let (api, env) = setup().await;
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/cloudflared")))
        .and(body_json(json!({"token": "cf-token-TEST"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector(
            "cloudflared",
            "provisioning",
            vec![],
        )))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "cloudflared", "enable"])
        .write_stdin("cf-token-TEST\n")
        .assert()
        .success();
}

#[test]
fn a_token_is_never_a_command_line_value() {
    let env = Env::new();
    env.cmd("http://unused")
        .args([
            "-p",
            PRJ,
            "network",
            "tailscale",
            "enable",
            "--auth-key",
            "tskey-auth-TEST",
        ])
        .assert()
        .failure()
        .code(2);
    env.cmd("http://unused")
        .args([
            "-p",
            PRJ,
            "network",
            "cloudflared",
            "enable",
            "--token",
            "cf-token-TEST",
        ])
        .assert()
        .failure()
        .code(2);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_token_file_that_others_can_read_is_refused() {
    let (api, env) = setup().await;
    let key = private_file(&env, "key", "tskey-auth-TEST\n", 0o644);
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/tailscale")))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "network",
            "tailscale",
            "enable",
            "--auth-key-file",
        ])
        .arg(&key)
        .assert()
        .failure()
        .stderr(predicate::str::contains("must be private"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disable_deletes_the_connector_after_confirmation() {
    let (api, env) = setup().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/tailscale")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "network", "tailscale", "disable"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Turning off the tailscale connector",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn expose_sends_the_target_to_each_connector_that_is_on() {
    let (api, env) = setup().await;
    mount_available(&api).await;
    mount_connectors(
        &api,
        vec![
            connector("tailscale", "connected", vec![]),
            connector("cloudflared", "connected", vec![]),
        ],
    )
    .await;
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/tailscale")))
        .and(body_json(json!({"targets": [
            {"kind": "container", "id": CTR, "port": 3000, "external_port": 3000}]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector(
            "tailscale",
            "connected",
            vec![],
        )))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/v1/projects/{PRJ}/connectors/cloudflared")))
        .and(body_json(json!({"targets": [
            {"kind": "container", "id": CTR, "port": 3000}]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector(
            "cloudflared",
            "connected",
            vec![],
        )))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "expose", "api"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn expose_keeps_the_other_targets_and_removes_nothing() {
    let (api, env) = setup().await;
    mount_available(&api).await;
    mount_connectors(
        &api,
        vec![connector(
            "tailscale",
            "connected",
            vec![target("database", DB, "main", 5432, Some(5432))],
        )],
    )
    .await;
    mount_put(
        &api,
        "tailscale",
        json!({"targets": [
        {"kind": "database", "id": DB, "port": 5432, "external_port": 5432},
        {"kind": "container", "id": CTR, "port": 3000, "external_port": 3000}]}),
        1,
    )
    .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "expose", "api", "--via", "tailscale"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn expose_needs_a_port_when_there_is_more_than_one() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/projects/{PRJ}/connectors/available-targets"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"targets": [
            {"kind": "container", "id": CTR, "name": "api", "ports": [3000, 8080]}]})))
        .mount(&api)
        .await;
    mount_connectors(&api, vec![connector("tailscale", "connected", vec![])]).await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "expose", "api", "--via", "tailscale"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--port"));
}

#[tokio::test(flavor = "multi_thread")]
async fn expose_with_an_unknown_name_or_a_connector_that_is_off_fails_without_a_change() {
    let (api, env) = setup().await;
    mount_available(&api).await;
    mount_connectors(&api, vec![connector("cloudflared", "connected", vec![])]).await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "expose", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No container or database named"));
    env.cmd(&api.uri())
        .args(["-p", PRJ, "network", "expose", "api", "--via", "tailscale"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Tailscale connector is off"));
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "network",
            "expose",
            "api",
            "--via",
            "cloudflared",
            "--external-port",
            "5000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Tailscale only"));
}

#[tokio::test(flavor = "multi_thread")]
async fn unexpose_removes_only_that_target() {
    let (api, env) = setup().await;
    mount_available(&api).await;
    mount_connectors(
        &api,
        vec![connector(
            "tailscale",
            "connected",
            vec![
                target("container", CTR, "api", 3000, Some(3000)),
                target("database", DB, "main", 5432, Some(5432)),
            ],
        )],
    )
    .await;
    mount_put(
        &api,
        "tailscale",
        json!({"targets": [
        {"kind": "database", "id": DB, "port": 5432, "external_port": 5432}]}),
        1,
    )
    .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "network",
            "unexpose",
            "api",
            "--via",
            "tailscale",
        ])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn unexpose_of_a_target_that_is_not_exposed_sends_nothing() {
    let (api, env) = setup().await;
    mount_available(&api).await;
    mount_connectors(&api, vec![connector("tailscale", "connected", vec![])]).await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "network",
            "unexpose",
            "main",
            "--via",
            "tailscale",
        ])
        .assert()
        .success();
}

#[test]
fn network_help_lists_the_commands() {
    let env = Env::new();
    env.cmd("http://unused")
        .args(["network", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("show")
                .and(predicate::str::contains("tailscale"))
                .and(predicate::str::contains("cloudflared"))
                .and(predicate::str::contains("expose"))
                .and(predicate::str::contains("unexpose")),
        );
}
