mod common;

use pms_client::ai_keys::{CreateAiKey, UpdateAiKey};
use pms_client::databases::CreateDatabase;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn db(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({
        "id": "db_abc", "name": "main", "engine": "postgres", "size": "pms-db-250m-512", "vcpu": 0.25,
        "memory_mib": 512, "vector": false, "status": "provisioning", "status_message": "Creating",
        "host": "db-abc.prj-a.svc", "port": 5432, "database": "app", "user": "app",
        "used_bytes": 0, "storage_limit_bytes": 1073741824,
        "created_at": "2026-10-10T10:00:00+00:00", "updated_at": "2026-10-10T10:00:00+00:00"});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[tokio::test]
async fn create_returns_the_password_one_time() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/databases"))
        .and(body_json(json!({"name": "main", "vector": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(db(json!({
            "password": "pw", "connection_url": "postgresql://app:pw@db-abc.prj-a.svc:5432/app"}))))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/databases/db_abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(db(json!({}))))
        .mount(&server)
        .await;
    let created = client
        .create_database(
            "prj_a",
            &CreateDatabase {
                name: "main".into(),
                size: None,
                vector: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.password.as_deref(), Some("pw"));
    assert!(created.connection_url.unwrap().contains("app:pw@"));
    let shown = client.database("prj_a", "db_abc").await.unwrap();
    assert_eq!(shown.password, None);
    assert_eq!(shown.storage_limit_bytes, 1073741824);
    // A secret that the API did not send is not invented in JSON output.
    assert!(!serde_json::to_string(&shown).unwrap().contains("password"));
}

#[tokio::test]
async fn list_vector_reset_delete() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/databases"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"items": [db(json!({"status": "running"}))]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a/databases/db_abc"))
        .and(body_json(json!({"vector": true})))
        .respond_with(ResponseTemplate::new(202).set_body_json(db(json!({"vector": true}))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/databases/db_abc/reset-password"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(db(json!({"password": "new", "connection_url": "u"}))),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/databases/db_abc"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    assert_eq!(
        client.databases("prj_a").await.unwrap()[0].status,
        "running"
    );
    assert!(
        client
            .set_database_vector("prj_a", "db_abc", true)
            .await
            .unwrap()
            .vector
    );
    assert_eq!(
        client
            .reset_database_password("prj_a", "db_abc")
            .await
            .unwrap()
            .password
            .as_deref(),
        Some("new")
    );
    client.delete_database("prj_a", "db_abc").await.unwrap();
}

#[tokio::test]
async fn vector_off_conflict_message() {
    let (server, client) = common::setup().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a/databases/db_abc"))
        .respond_with(ResponseTemplate::new(409).set_body_json(
            json!({"error": {"code": "conflict", "message": "A column or index uses vector. Remove it first."}}),
        ))
        .mount(&server)
        .await;
    let err = client
        .set_database_vector("prj_a", "db_abc", false)
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "A column or index uses vector. Remove it first."
    );
}

fn key(extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({"id": "key_abc", "name": "app", "status": "active", "prefix": "pms_sk_ab",
        "models": ["gemma4-e4b"], "budget_usd": 5.0, "spend_usd": 0.25, "rate_limit_rpm": 60,
        "created_at": "2026-10-10T10:00:00+00:00"});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[tokio::test]
async fn ai_key_lifecycle() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/ai-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [key(json!({}))]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/ai-keys"))
        .and(body_json(
            json!({"name": "app", "models": ["gemma4-e4b"], "budget_usd": 5.0}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(key(json!({"secret": "pms_sk_full"}))),
        )
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/prj_a/ai-keys/key_abc"))
        .and(body_json(json!({"budget_usd": 10.0})))
        .respond_with(ResponseTemplate::new(200).set_body_json(key(json!({"budget_usd": 10.0}))))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/prj_a/ai-keys/key_abc/rotate"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(key(json!({"secret": "pms_sk_new"}))),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1/projects/prj_a/ai-keys/key_abc"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let list = client.ai_keys("prj_a").await.unwrap();
    assert_eq!(list[0].spend_usd, 0.25);
    assert_eq!(list[0].rate_limit_rpm, Some(60));
    let created = client
        .create_ai_key(
            "prj_a",
            &CreateAiKey {
                name: "app".into(),
                models: vec!["gemma4-e4b".into()],
                budget_usd: 5.0,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.secret.as_deref(), Some("pms_sk_full"));
    let edited = client
        .update_ai_key(
            "prj_a",
            "key_abc",
            &UpdateAiKey {
                budget_usd: Some(10.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.budget_usd, 10.0);
    assert_eq!(
        client
            .rotate_ai_key("prj_a", "key_abc")
            .await
            .unwrap()
            .secret
            .as_deref(),
        Some("pms_sk_new")
    );
    client.delete_ai_key("prj_a", "key_abc").await.unwrap();
}
