mod common;

use common::{fake_bin, vm_json, Env, PRJ};
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup() -> (MockServer, Env) {
    let api = MockServer::start().await;
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    (api, env)
}

async fn mount_list(api: &MockServer, status: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [vm_json(status)]})))
        .mount(api)
        .await;
}

async fn mount_vm(api: &MockServer, status: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456")))
        .respond_with(ResponseTemplate::new(200).set_body_json(vm_json(status)))
        .mount(api)
        .await;
}

/// The ticket route and a stream that has one event for the VM.
async fn mount_stream(api: &MockServer, events: &[serde_json::Value]) {
    Mock::given(method("POST"))
        .and(path("/v1/events/ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticket": "tkt"})))
        .mount(api)
        .await;
    let body: String = events
        .iter()
        .map(|e| {
            format!(
                "id: {}\nevent: event\ndata: {e}\n\n",
                e["id"].as_str().unwrap()
            )
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(api)
        .await;
}

fn vm_event(id: &str, message: &str) -> serde_json::Value {
    json!({"id": id, "time": "2026-10-10T10:00:00+00:00", "level": "info",
           "resource_id": "vm_abc123def456", "resource_name": "web", "message": message,
           "project_id": PRJ})
}

#[tokio::test(flavor = "multi_thread")]
async fn list_show_by_name_and_unknown_name() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    mount_vm(&api, "running").await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("vm_abc123def456").and(predicate::str::contains("10.0.0.5")),
        );
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "show", "web"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:").and(predicate::str::contains("running")));
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "-o", "json", "vm", "show", "vm_abc123def456"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ssh_user"], "ubuntu");
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "show", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No VM named"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_needs_exactly_one_key_source() {
    let env = Env::new();
    env.sign_in("https://unused.example", 3600);
    let base = [
        "-p",
        PRJ,
        "vm",
        "create",
        "web",
        "--size",
        "pms-s-1",
        "--image",
        "ubuntu-24.04",
    ];
    env.cmd("http://unused")
        .args(base)
        .assert()
        .failure()
        .code(2);
    env.cmd("http://unused")
        .args(base)
        .args(["--generate-key", "--ssh-key", "laptop"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn create_with_a_public_key_file_sends_the_exact_body() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .and(wiremock::matchers::body_json(json!({
            "name": "web", "size": "pms-s-1", "image": "ubuntu-24.04", "disk_gb": 20,
            "ssh_public_key": "ssh-ed25519 AAAA me"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    let key = env.dir.path().join("k.pub");
    std::fs::write(&key, "ssh-ed25519 AAAA me\n").unwrap();
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "pms-s-1",
            "--image",
            "ubuntu-24.04",
            "--disk-gb",
            "20",
            "--public-key-file",
        ])
        .arg(&key)
        .assert()
        .success()
        .stdout(predicate::str::contains("provisioning"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_with_a_saved_key_by_name() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/ssh-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "ssh_1", "name": "laptop", "public_key": "ssh-ed25519 SAVED x",
             "fingerprint": "SHA256:a", "created_at": "2026-10-08T10:00:00+00:00"}]})))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .and(body_partial_json(
            json!({"ssh_public_key": "ssh-ed25519 SAVED x"}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "s",
            "--image",
            "i",
            "--ssh-key",
            "laptop",
        ])
        .assert()
        .success();
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "s",
            "--image",
            "i",
            "--ssh-key",
            "missing",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No saved SSH key"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn generate_key_writes_the_private_key_locally_and_sends_only_the_public_key() {
    use std::os::unix::fs::PermissionsExt;
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("provisioning")))
        .expect(1)
        .mount(&api)
        .await;
    let out = env.dir.path().join("keys").join("web");
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "s",
            "--image",
            "i",
            "--generate-key",
            "--key-out",
        ])
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains("Private key saved"));
    let private = std::fs::read_to_string(&out).unwrap();
    let public = std::fs::read_to_string(format!("{}.pub", out.display())).unwrap();
    assert!(private.contains("OPENSSH PRIVATE KEY"));
    assert_eq!(
        std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let sent = api.received_requests().await.unwrap();
    let post = sent.iter().find(|r| r.method.as_str() == "POST").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
    assert_eq!(body["ssh_public_key"].as_str().unwrap(), public.trim());
    assert!(!String::from_utf8_lossy(&post.body).contains("PRIVATE KEY"));
    // A second run does not overwrite the key.
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web2",
            "--size",
            "s",
            "--image",
            "i",
            "--generate-key",
            "--key-out",
        ])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("exists"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn failed_create_removes_the_generated_key() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"error": {
            "code": "quota_exceeded", "message": "You can have 1 VM. Delete a VM first."}})))
        .mount(&api)
        .await;
    let out = env.dir.path().join("web-key");
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "s",
            "--image",
            "i",
            "--generate-key",
            "--key-out",
        ])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("You can have 1 VM"));
    assert!(!out.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn create_wait_follows_the_vm_until_it_runs() {
    let (api, env) = setup().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms")))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("provisioning")))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456")))
        .respond_with(ResponseTemplate::new(200).set_body_json(vm_json("provisioning")))
        .up_to_n_times(1)
        .mount(&api)
        .await;
    mount_vm(&api, "running").await;
    mount_stream(&api, &[vm_event("evt_1", "VM running")]).await;
    env.cmd(&api.uri())
        .args([
            "-p",
            PRJ,
            "vm",
            "create",
            "web",
            "--size",
            "s",
            "--image",
            "i",
            "--public-key-file",
        ])
        .arg({
            let p = env.dir.path().join("k.pub");
            std::fs::write(&p, "ssh-ed25519 AAAA me\n").unwrap();
            p
        })
        .args(["--wait", "--timeout", "30"])
        .assert()
        .success()
        .stdout(predicate::str::contains("running"))
        .stderr(predicate::str::contains("provisioning"));
}

#[tokio::test(flavor = "multi_thread")]
async fn wait_reports_a_failed_vm() {
    let (api, env) = setup().await;
    mount_list(&api, "stopped").await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1/projects/{PRJ}/vms/vm_abc123def456/actions"
        )))
        .and(body_partial_json(json!({"action": "start"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("provisioning")))
        .mount(&api)
        .await;
    let mut failed = vm_json("failed");
    failed["status_message"] = json!("The disk is missing.");
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456")))
        .respond_with(ResponseTemplate::new(200).set_body_json(failed))
        .mount(&api)
        .await;
    mount_stream(&api, &[]).await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "start", "web", "--wait"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed: The disk is missing."));
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_and_restart_and_timeout_flag_needs_wait() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    for action in ["stop", "restart"] {
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/{PRJ}/vms/vm_abc123def456/actions"
            )))
            .and(body_partial_json(json!({"action": action})))
            .respond_with(ResponseTemplate::new(202).set_body_json(vm_json("running")))
            .expect(1)
            .mount(&api)
            .await;
        env.cmd(&api.uri())
            .args(["-p", PRJ, "vm", action, "web"])
            .assert()
            .success();
    }
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "stop", "web", "--timeout", "5"])
        .assert()
        .failure()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_asks_for_yes_and_waits_for_not_found() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456")))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "delete", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456")))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"error": {"code": "not_found", "message": "VM not found."}})),
        )
        .mount(&api)
        .await;
    mount_stream(&api, &[]).await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "-y", "vm", "delete", "web", "--wait"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted VM web"));
}

#[tokio::test(flavor = "multi_thread")]
async fn console_refuses_a_stopped_vm_and_a_missing_terminal() {
    let (api, env) = setup().await;
    mount_list(&api, "stopped").await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "console", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs a running VM"));
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/projects/{PRJ}/vms/vm_abc123def456/console/ticket")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ticket": "T.S", "url": format!("/v1/projects/{PRJ}/vms/vm_abc123def456/console?ticket=T.S")})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "console", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs a terminal"));
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_with_no_data() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/projects/{PRJ}/vms/vm_abc123def456/metrics"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"cpu": []})))
        .mount(&api)
        .await;
    env.cmd(&api.uri())
        .args(["-p", PRJ, "vm", "metrics", "web"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No metrics yet"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ssh_runs_ssh_with_a_cloudflared_proxy_command() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    let bin = env.dir.path().join("bin");
    fake_bin(&bin, "ssh", "for a in \"$@\"; do echo \"ARG:$a\"; done");
    fake_bin(&bin, "cloudflared", "exit 0");
    let out = env
        .cmd(&api.uri())
        .env("PATH", &bin)
        .args([
            "-p", PRJ, "vm", "ssh", "web", "-i", "/keys/id", "--", "uptime",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("ARG:HostKeyAlias=vm_abc123def456"));
    assert!(text.contains("ARG:ubuntu@10.0.0.5"));
    assert!(text.contains("ARG:uptime"));
    assert!(text.contains(&format!(
        "ProxyCommand=ssh -o 'ProxyCommand={}/cloudflared access ssh --hostname jump.example.org'",
        bin.display()
    )));
    assert!(text.contains("-W %h:%p -l jump jump.example.org"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ssh_without_cloudflared_asks_before_downloading() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    let bin = env.dir.path().join("bin");
    fake_bin(&bin, "ssh", "echo should-not-run");
    env.cmd(&api.uri())
        .env("PATH", &bin)
        .args(["-p", PRJ, "vm", "ssh", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cloudflared").and(predicate::str::contains("--yes")));
    assert!(!env.dir.path().join("data").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn ssh_to_a_stopped_vm_does_not_offer_a_download() {
    let (api, env) = setup().await;
    mount_list(&api, "stopped").await;
    env.cmd(&api.uri())
        .env("PATH", env.dir.path())
        .args(["-p", PRJ, "vm", "ssh", "web"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must be running"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ssh_config_prints_the_console_block() {
    let (api, env) = setup().await;
    mount_list(&api, "running").await;
    env.cmd(&api.uri())
        .env("PATH", env.dir.path())
        .args(["-p", PRJ, "ssh-config"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Host pms-jump\n  HostName jump.example.org\n  User jump\n")
                .and(predicate::str::contains(
                    "ProxyCommand cloudflared access ssh --hostname %h",
                ))
                .and(predicate::str::contains(
                    "Host pms-web\n  HostName 10.0.0.5\n  User ubuntu\n  ProxyJump pms-jump",
                )),
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn events_list_is_oldest_first() {
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/events")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "evt_2", "time": "2026-10-10T10:02:00+00:00", "level": "success", "resource_id": "vm_1",
             "resource_name": "web", "message": "VM running"},
            {"id": "evt_1", "time": "2026-10-10T10:01:00+00:00", "level": "info", "resource_id": "vm_1",
             "resource_name": "web", "message": "Creating VM"}]})))
        .mount(&api)
        .await;
    let out = env
        .cmd(&api.uri())
        .args(["-p", PRJ, "events"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.find("Creating VM").unwrap() < text.find("VM running").unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn events_follow_prints_new_events_of_this_project_only() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let (api, env) = setup().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/events")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
        .mount(&api)
        .await;
    let mut other = vm_event("evt_8", "Other project event");
    other["project_id"] = json!("prj_zzzzzzzzzzzz");
    mount_stream(&api, &[other, vm_event("evt_9", "VM running")]).await;
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("pms"))
        .env("PMS_CONFIG_DIR", env.config_dir())
        .env("PMS_NO_KEYRING", "1")
        .args([
            "--api-url",
            &api.uri(),
            "-p",
            PRJ,
            "-o",
            "json",
            "events",
            "--follow",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["id"], "evt_9");
}
