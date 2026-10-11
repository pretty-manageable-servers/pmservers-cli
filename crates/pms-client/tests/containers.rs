mod common;

use pms_client::containers::{
    CreateContainer, CreateRegistryCredential, PowerAction, UpdateContainer,
};
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, ResponseTemplate};

fn container(status: &str) -> serde_json::Value {
    json!({"id": "ctr_abc", "name": "api", "type": "worker", "image": "ghcr.io/o/app:1",
           "size": "pms-c-250m-512", "runtime": "gvisor", "vcpu": 0.25, "memory_mib": 512,
           "registry_credential_id": null, "env_names": ["A", "B"], "ports": [8080], "internal_host": "api", "status": status,
           "status_message": null, "created_at": "2026-10-10T10:00:00+00:00",
           "updated_at": "2026-10-10T10:00:00+00:00"})
}

#[tokio::test]
async fn access_state_and_request() {
    let (server, client) = common::setup().await;
    let body = json!({"access": "none", "runtime": "gvisor", "vcluster": "none", "reason": null,
                      "note": null, "requested_at": null, "decided_at": null});
    Mock::given(method("GET"))
        .and(path("/v1/account/access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;
    let mut pending = body;
    pending["access"] = json!("pending");
    pending["reason"] = json!("Batch jobs");
    Mock::given(method("POST"))
        .and(path("/v1/account/access/request"))
        .and(body_json(json!({"reason": "Batch jobs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(pending))
        .mount(&server)
        .await;
    assert_eq!(client.access().await.unwrap().access, "none");
    let a = client.request_access("Batch jobs").await.unwrap();
    assert_eq!(a.access, "pending");
    assert_eq!(a.reason.as_deref(), Some("Batch jobs"));
}

#[tokio::test]
async fn list_and_403_for_an_unapproved_owner() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/containers"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"items": [container("running")]})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/containers"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": "not_allowed", "message": "Containers need approval for the project owner's account."}})))
        .mount(&server)
        .await;
    let list = client.containers("prj_a").await.unwrap();
    assert_eq!(list[0].kind, "worker");
    assert_eq!(list[0].env_names, ["A", "B"]);
    assert_eq!(list[0].ports, [8080]);
    assert_eq!(list[0].internal_host.as_deref(), Some("api"));
    assert_eq!(list[0].vcpu, Some(0.25));
    let err = client.containers("prj_a").await.unwrap_err();
    assert_eq!(err.status(), Some(403));
}

#[tokio::test]
async fn create_body_matches_the_api() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/containers"))
        .and(body_json(
            json!({"name": "api", "type": "worker", "image": "ghcr.io/o/app:1",
                              "size": "pms-c-250m-512", "registry_credential_id": "reg_1",
                              "env": {"A": "1"}, "runtime": "native", "ports": [8080, 9000]}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(container("provisioning")))
        .expect(1)
        .mount(&server)
        .await;
    let c = client
        .create_container(
            "prj_a",
            &CreateContainer {
                name: "api".into(),
                kind: "worker".into(),
                image: "ghcr.io/o/app:1".into(),
                size: "pms-c-250m-512".into(),
                registry_credential_id: Some("reg_1".into()),
                env: [("A".to_string(), "1".to_string())].into(),
                runtime: Some("native".into()),
                ports: vec![8080, 9000],
                tunnel_port: None,
                tunnel_token: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(c.status, "provisioning");
}

#[tokio::test]
async fn update_sends_only_the_fields_that_are_set() {
    let (server, client) = common::setup().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc"))
        .and(body_json(json!({"image": "ghcr.io/o/app:2"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(container("provisioning")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc"))
        .and(body_json(
            json!({"registry_credential_id": null, "env": {}}),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_json(container("provisioning")))
        .expect(1)
        .mount(&server)
        .await;
    client
        .update_container(
            "prj_a",
            "ctr_abc",
            &UpdateContainer {
                image: Some("ghcr.io/o/app:2".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    client
        .update_container(
            "prj_a",
            "ctr_abc",
            &UpdateContainer {
                registry_credential_id: Some(None),
                env: Some(Default::default()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn actions_delete_and_logs() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc/actions"))
        .and(body_json(json!({"action": "stop"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(container("stopped")))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc/logs"))
        .and(query_param("tail", "50"))
        .and(query_param_is_missing("previous"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"lines": [
            {"time": "2026-10-10T10:00:00.1Z", "message": "hello"}]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/containers/ctr_abc/logs"))
        .and(query_param("previous", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"lines": []})))
        .mount(&server)
        .await;
    assert_eq!(
        client
            .container_action("prj_a", "ctr_abc", PowerAction::Stop)
            .await
            .unwrap()
            .status,
        "stopped"
    );
    client.delete_container("prj_a", "ctr_abc").await.unwrap();
    let lines = client
        .container_logs("prj_a", "ctr_abc", 50, false)
        .await
        .unwrap();
    assert_eq!(lines[0].message, "hello");
    assert!(client
        .container_logs("prj_a", "ctr_abc", 200, true)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn registry_credentials() {
    let (server, client) = common::setup().await;
    let cred = json!({"id": "reg_1", "name": "ghcr", "host": "ghcr.io", "username": "bot",
                      "created_at": "2026-10-10T10:00:00+00:00"});
    Mock::given(method("GET"))
        .and(path("/v1/registry-credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [cred.clone()]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/registry-credentials"))
        .and(body_json(
            json!({"name": "ghcr", "host": "ghcr.io", "username": "bot", "token": "s3cret"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(cred))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/registry-credentials/reg_1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    assert_eq!(
        client.registry_credentials().await.unwrap()[0].host,
        "ghcr.io"
    );
    client
        .add_registry_credential(&CreateRegistryCredential {
            name: "ghcr".into(),
            host: "ghcr.io".into(),
            username: "bot".into(),
            token: "s3cret".into(),
        })
        .await
        .unwrap();
    client.delete_registry_credential("reg_1").await.unwrap();
}
