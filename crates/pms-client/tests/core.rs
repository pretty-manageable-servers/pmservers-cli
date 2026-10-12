mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn project(id: &str, name: &str, role: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "region": "ca-tor-1", "role": role,
           "owner_id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90",
           "created_at": "2026-10-09T10:00:00+00:00"})
}

#[tokio::test]
async fn list_and_create_projects() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"items": [project("prj_a", "one", "owner"), project("prj_b", "two", "member")]}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects"))
        .and(body_json(json!({"name": "new"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(project("prj_c", "new", "owner")))
        .mount(&server)
        .await;
    let list = client.projects().await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].role, "member");
    assert_eq!(client.create_project("new").await.unwrap().id, "prj_c");
}

#[tokio::test]
async fn rename_and_delete_project() {
    let (server, client) = common::setup().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a"))
        .and(body_json(json!({"name": "renamed"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(project("prj_a", "renamed", "owner")),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    assert_eq!(
        client
            .rename_project("prj_a", "renamed")
            .await
            .unwrap()
            .name,
        "renamed"
    );
    client.delete_project("prj_a").await.unwrap();
}

#[tokio::test]
async fn delete_project_conflict_message() {
    let (server, client) = common::setup().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"error": {
            "code": "conflict",
            "message": "Delete the containers, databases and AI keys of this project first."}})))
        .mount(&server)
        .await;
    let err = client.delete_project("prj_a").await.unwrap_err();
    assert_eq!(err.code(), Some("conflict"));
}

#[tokio::test]
async fn usage_shape() {
    let (server, client) = common::setup().await;
    let zeros = vec![0; 28];
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "vcpu": {"used": 1.25, "limit": 2, "series": zeros},
            "memory_gib": {"used": 2.5, "limit": 4, "series": zeros},
            "ai_spend_usd": {"used": 0.5, "limit": 5.0, "series": zeros}
        })))
        .mount(&server)
        .await;
    let u = client.usage("prj_a").await.unwrap();
    assert_eq!(u.vcpu.used, 1.25);
    assert_eq!(u.ai_spend_usd.limit, 5.0);
    assert_eq!(u.vcpu.series.len(), 28);
}

#[tokio::test]
async fn members_invite_and_remove() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"user_id": "u1", "email": "o@x.co", "role": "owner", "created_at": "2026-10-09T10:00:00+00:00"},
            {"user_id": "u2", "email": "m@x.co", "role": "member", "created_at": "2026-10-09T11:00:00+00:00"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/members"))
        .and(body_json(json!({"email": "n@x.co"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "user_id": "u3", "email": "n@x.co", "role": "member",
            "created_at": "2026-10-10T10:00:00+00:00", "invited": true})))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/members/u2"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(client.members("prj_a").await.unwrap()[1].email, "m@x.co");
    assert_eq!(
        client
            .invite_member("prj_a", "n@x.co")
            .await
            .unwrap()
            .invited,
        Some(true)
    );
    client.remove_member("prj_a", "u2").await.unwrap();
}

#[tokio::test]
async fn ids_are_escaped_in_paths() {
    let (server, client) = common::setup().await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/members/a%2Fb"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    client.remove_member("prj_a", "a/b").await.unwrap();
}
