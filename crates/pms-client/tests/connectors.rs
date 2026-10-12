mod common;

use pms_client::connectors::{PutConnector, TargetInput};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn connector(kind: &str, status: &str) -> serde_json::Value {
    json!({"id": "ncn_abc", "kind": kind, "status": status, "message": null,
           "observed_at": null,
           "targets": [{"kind": "container", "id": "ctr_abc", "name": "api", "port": 3000,
                        "external_port": 3000, "address": "prj-a.tn.ts.net:3000"}],
           "created_at": "2026-10-11T10:00:00+00:00", "updated_at": "2026-10-11T10:00:00+00:00"})
}

#[tokio::test]
async fn list_connectors_and_their_targets() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/connectors"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "connectors": [connector("tailscale", "connected")]})))
        .expect(1)
        .mount(&server)
        .await;
    let list = client.connectors("prj_a").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, "tailscale");
    assert_eq!(list[0].status, "connected");
    assert_eq!(list[0].targets[0].external_port, Some(3000));
    assert_eq!(
        list[0].targets[0].address.as_deref(),
        Some("prj-a.tn.ts.net:3000")
    );
}

#[tokio::test]
async fn available_targets_have_ports() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/connectors/available-targets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"targets": [
            {"kind": "container", "id": "ctr_abc", "name": "api", "ports": [3000, 8080]},
            {"kind": "database", "id": "db_abc", "name": "main", "ports": [5432]}]})))
        .expect(1)
        .mount(&server)
        .await;
    let targets = client.available_targets("prj_a").await.unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].ports, vec![3000, 8080]);
    assert_eq!(targets[1].kind, "database");
    assert_eq!(targets[1].ports, vec![5432]);
}

#[tokio::test]
async fn put_with_token_makes_the_connector() {
    let (server, client) = common::setup().await;
    Mock::given(method("PUT"))
        .and(path("/v1/projects/prj_a/connectors/cloudflared"))
        .and(body_json(json!({"token": "cf-token-TEST"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(connector("cloudflared", "provisioning")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client
        .put_connector(
            "prj_a",
            "cloudflared",
            &PutConnector {
                token: Some("cf-token-TEST".into()),
                targets: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(c.status, "provisioning");
}

#[tokio::test]
async fn put_targets_only_keeps_the_token_out_of_the_body() {
    let (server, client) = common::setup().await;
    Mock::given(method("PUT"))
        .and(path("/v1/projects/prj_a/connectors/tailscale"))
        .and(body_json(json!({"targets": [
            {"kind": "container", "id": "ctr_abc", "port": 3000, "external_port": 3001},
            {"kind": "database", "id": "db_abc", "port": 5432}]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector("tailscale", "connected")))
        .expect(1)
        .mount(&server)
        .await;
    let c = client
        .put_connector(
            "prj_a",
            "tailscale",
            &PutConnector {
                token: None,
                targets: Some(vec![
                    TargetInput {
                        kind: "container".into(),
                        id: "ctr_abc".into(),
                        port: 3000,
                        external_port: Some(3001),
                    },
                    TargetInput {
                        kind: "database".into(),
                        id: "db_abc".into(),
                        port: 5432,
                        external_port: None,
                    },
                ]),
            },
        )
        .await
        .unwrap();
    assert_eq!(c.kind, "tailscale");
}

#[tokio::test]
async fn put_with_empty_targets_sends_an_empty_list() {
    let (server, client) = common::setup().await;
    Mock::given(method("PUT"))
        .and(path("/v1/projects/prj_a/connectors/tailscale"))
        .and(body_json(json!({"targets": []})))
        .respond_with(ResponseTemplate::new(200).set_body_json(connector("tailscale", "connected")))
        .expect(1)
        .mount(&server)
        .await;
    client
        .put_connector(
            "prj_a",
            "tailscale",
            &PutConnector {
                token: None,
                targets: Some(vec![]),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn put_error_shows_the_api_message() {
    let (server, client) = common::setup().await;
    Mock::given(method("PUT"))
        .and(path("/v1/projects/prj_a/connectors/tailscale"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({"error": {
            "code": "validation_error",
            "message": "A Tailscale auth key starts with tskey-."}})))
        .expect(1)
        .mount(&server)
        .await;
    let err = client
        .put_connector(
            "prj_a",
            "tailscale",
            &PutConnector {
                token: Some("bad".into()),
                targets: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some("validation_error"));
    assert_eq!(err.to_string(), "A Tailscale auth key starts with tskey-.");
}

#[tokio::test]
async fn delete_connector_accepts_204() {
    let (server, client) = common::setup().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/connectors/cloudflared"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    client
        .delete_connector("prj_a", "cloudflared")
        .await
        .unwrap();
}
