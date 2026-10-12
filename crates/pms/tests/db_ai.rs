mod common;

use common::{fake_bin, Env, PRJ};
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

// --- pms db connect ---

async fn mount_connectable(api: &MockServer, extra: serde_json::Value) {
    let mut fields = json!({"private_ip": "10.43.0.9", "ssh_host": "jump.example.org"});
    fields
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    Mock::given(method("GET"))
        .and(path(format!("/v1/projects/{PRJ}/databases")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [db(fields)]})))
        .mount(api)
        .await;
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_refuses_a_database_that_cannot_be_reached() {
    for (extra, text) in [
        (json!({"status": "provisioning"}), "must be running"),
        (json!({"private_ip": null}), "no private IP"),
        (json!({"private_ip": ""}), "no private IP"),
        (json!({"ssh_host": ""}), "no SSH jump host"),
    ] {
        let (api, env) = setup().await;
        mount_connectable(&api, extra).await;
        // No ssh and no cloudflared on PATH: the checks come first, and nothing is offered for download.
        env.cmd(&api.uri())
            .env("PATH", env.dir.path())
            .args(["-p", PRJ, "db", "connect", "main"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(text));
        assert!(!env.dir.path().join("data").exists());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_refuses_a_port_that_is_taken() {
    let (api, env) = setup().await;
    mount_connectable(&api, json!({})).await;
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    env.cmd(&api.uri())
        .env("PATH", env.dir.path())
        .args(["-p", PRJ, "db", "connect", "main", "--port", &port])
        .assert()
        .failure()
        .stderr(predicate::str::contains(format!(
            "Local port {port} is not free"
        )));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn connect_without_cloudflared_asks_before_downloading() {
    let (api, env) = setup().await;
    mount_connectable(&api, json!({})).await;
    let bin = env.dir.path().join("bin");
    fake_bin(&bin, "ssh", "echo should-not-run");
    env.cmd(&api.uri())
        .env("PATH", &bin)
        .args([
            "-p",
            PRJ,
            "db",
            "connect",
            "main",
            "--port",
            &free_port().to_string(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cloudflared").and(predicate::str::contains("--yes")));
    assert!(!env.dir.path().join("data").exists());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn connect_says_so_when_ssh_stops_early() {
    let (api, env) = setup().await;
    mount_connectable(&api, json!({})).await;
    let bin = env.dir.path().join("bin");
    fake_bin(
        &bin,
        "ssh",
        "for a in \"$@\"; do echo \"ARG:$a\"; done; exit 255",
    );
    fake_bin(&bin, "cloudflared", "exit 0");
    let port = free_port();
    env.cmd(&api.uri())
        .env("PATH", &bin)
        .args([
            "-p",
            PRJ,
            "db",
            "connect",
            "main",
            "--port",
            &port.to_string(),
            "-i",
            "/keys/id",
        ])
        .assert()
        .failure()
        .stdout(
            predicate::str::contains(format!("ARG:127.0.0.1:{port}:10.43.0.9:5432"))
                .and(predicate::str::contains("ARG:-N"))
                .and(predicate::str::contains("ARG:/keys/id"))
                .and(predicate::str::contains(format!(
                    "ARG:ProxyCommand={}/cloudflared access ssh --hostname jump.example.org",
                    bin.display()
                )))
                .and(predicate::str::contains("ARG:jump.example.org")),
        )
        .stderr(predicate::str::contains(
            "ssh stopped before the tunnel was ready",
        ));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn connect_prints_the_url_and_stops_on_ctrl_c() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let (api, env) = setup().await;
    mount_connectable(&api, json!({})).await;
    let bin = env.dir.path().join("bin");
    // The fake ssh does not listen. The test listens on the port, as ssh would after it logs in.
    fake_bin(
        &bin,
        "ssh",
        "/bin/sleep 5 & trap '/bin/kill $!; exit 0' TERM; wait",
    );
    fake_bin(&bin, "cloudflared", "exit 0");
    let port = free_port();
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("pms"))
        .env("PMS_CONFIG_DIR", env.config_dir())
        .env("PMS_DATA_DIR", env.dir.path().join("data"))
        .env("PMS_NO_KEYRING", "1")
        .env("PMS_SKIP_KEYCHAIN_MOVE", "1")
        .env("PMS_API_URL", api.uri())
        .env("PATH", &bin)
        .args([
            "-p",
            PRJ,
            "db",
            "connect",
            "main",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(
        line.trim(),
        format!("postgres://app:<password>@localhost:{port}/app")
    );
    std::process::Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    drop(listener);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Stopped."));
}
