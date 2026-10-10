mod common;

use pms_client::vms::{CreateVm, PowerAction};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn vm(status: &str) -> serde_json::Value {
    json!({"id": "vm_abc", "name": "web", "status": status, "status_message": null,
           "size": "pms-s-1", "image": "ubuntu-24.04", "disk_gb": 10, "private_ip": "10.0.0.5",
           "ssh_host": "jump.example.org", "ssh_user": "ubuntu", "created_at": "2026-10-09T10:00:00+00:00"})
}

#[tokio::test]
async fn list_and_show() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/vms"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [vm("running")]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/vms/vm_abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vm("running")))
        .mount(&server)
        .await;
    let list = client.vms("prj_a").await.unwrap();
    assert_eq!(list[0].private_ip.as_deref(), Some("10.0.0.5"));
    assert_eq!(
        client.vm("prj_a", "vm_abc").await.unwrap().ssh_user,
        "ubuntu"
    );
}

#[tokio::test]
async fn create_sends_the_exact_body_and_reads_202() {
    let (server, client) = common::setup().await;
    let mut created = vm("provisioning");
    created["status_message"] = json!("Creating");
    created["private_ip"] = json!(null);
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/vms"))
        .and(body_json(
            json!({"name": "web", "size": "pms-s-1", "image": "ubuntu-24.04",
                              "disk_gb": 10, "ssh_public_key": "ssh-ed25519 AAAA me"}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(created))
        .expect(1)
        .mount(&server)
        .await;
    let vm = client
        .create_vm(
            "prj_a",
            &CreateVm {
                name: "web".into(),
                size: "pms-s-1".into(),
                image: "ubuntu-24.04".into(),
                disk_gb: 10,
                ssh_public_key: "ssh-ed25519 AAAA me".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(vm.status, "provisioning");
    assert_eq!(vm.private_ip, None);
}

#[tokio::test]
async fn actions_and_delete() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/vms/vm_abc/actions"))
        .and(body_json(json!({"action": "restart"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(vm("running")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/vms/vm_abc"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    client
        .vm_action("prj_a", "vm_abc", PowerAction::Restart)
        .await
        .unwrap();
    client.delete_vm("prj_a", "vm_abc").await.unwrap();
}

#[tokio::test]
async fn action_conflict_is_an_api_error() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/vms/vm_abc/actions"))
        .respond_with(ResponseTemplate::new(409).set_body_json(
            json!({"error": {"code": "conflict", "message": "The VM is not available for this action now."}}),
        ))
        .mount(&server)
        .await;
    let err = client
        .vm_action("prj_a", "vm_abc", PowerAction::Start)
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(409));
}

#[tokio::test]
async fn metrics_stub_shape() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/vms/vm_abc/metrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"cpu": []})))
        .mount(&server)
        .await;
    assert!(client
        .vm_metrics("prj_a", "vm_abc")
        .await
        .unwrap()
        .cpu
        .is_empty());
}

#[tokio::test]
async fn console_ticket_and_ws_url() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/vms/vm_abc/console/ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ticket": "T.S",
            "url": "/v1/projects/prj_a/vms/vm_abc/console?ticket=T.S"})))
        .mount(&server)
        .await;
    let t = client.console_ticket("prj_a", "vm_abc").await.unwrap();
    let url = client.console_ws_url(&t).unwrap();
    assert_eq!(url.scheme(), "ws");
    assert_eq!(url.path(), "/v1/projects/prj_a/vms/vm_abc/console");
    assert_eq!(url.query(), Some("ticket=T.S"));
}

#[tokio::test]
async fn console_ticket_refused_when_vm_is_not_running() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/vms/vm_abc/console/ticket"))
        .respond_with(ResponseTemplate::new(409).set_body_json(
            json!({"error": {"code": "conflict", "message": "The VM must be running to open the console."}}),
        ))
        .mount(&server)
        .await;
    let err = client.console_ticket("prj_a", "vm_abc").await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "The VM must be running to open the console."
    );
}
