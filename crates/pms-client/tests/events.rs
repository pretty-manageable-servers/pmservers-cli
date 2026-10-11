mod common;

use std::time::Duration;

use pms_client::events::EventFollower;
use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, ResponseTemplate};

fn event(id: &str, message: &str) -> serde_json::Value {
    json!({"id": id, "time": "2026-10-10T10:00:00+00:00", "level": "success",
           "resource_id": "ctr_abc", "resource_name": "web", "message": message,
           "project_id": "prj_a"})
}

fn sse(id: &str, message: &str) -> String {
    format!("id: {id}\nevent: event\ndata: {}\n\n", event(id, message))
}

fn stream_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

#[tokio::test]
async fn list_events() {
    let (server, client) = common::setup().await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/prj_a/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "evt_2", "time": "2026-10-10T10:01:00+00:00", "level": "info",
             "resource_id": "ctr_abc", "resource_name": "web", "message": "Starting container"}
        ]})))
        .mount(&server)
        .await;
    let events = client.events("prj_a").await.unwrap();
    assert_eq!(events[0].project_id, None);
    assert_eq!(events[0].message, "Starting container");
}

#[tokio::test]
async fn follower_reconnects_with_last_event_id() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/events/ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticket": "tkt"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .and(query_param("ticket", "tkt"))
        .and(query_param_is_missing("last_event_id"))
        .respond_with(stream_response(format!(
            ": connected\n\n{}",
            sse("evt_1", "Creating container")
        )))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .and(query_param("last_event_id", "evt_1"))
        .respond_with(stream_response(sse("evt_2", "Container running")))
        .expect(1)
        .mount(&server)
        .await;
    let mut follower = EventFollower::new(client, None).with_base_delay(Duration::from_millis(10));
    assert_eq!(follower.next().await.unwrap().id, "evt_1");
    // The first stream ended. The follower asks for what it missed.
    let second = follower.next().await.unwrap();
    assert_eq!(second.id, "evt_2");
    assert_eq!(second.message, "Container running");
    assert_eq!(follower.last_event_id(), Some("evt_2"));
}

#[tokio::test]
async fn follower_retries_after_a_server_error() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/events/ticket"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticket": "tkt"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/events/stream"))
        .respond_with(stream_response(sse("evt_9", "ok")))
        .mount(&server)
        .await;
    let mut follower = EventFollower::new(client, None).with_base_delay(Duration::from_millis(5));
    assert_eq!(follower.next().await.unwrap().id, "evt_9");
}

#[tokio::test]
async fn follower_stops_on_forbidden() {
    let (server, client) = common::setup().await;
    Mock::given(method("POST"))
        .and(path("/v1/events/ticket"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": {"code": "not_allowed", "message": "No."}})),
        )
        .mount(&server)
        .await;
    let mut follower = EventFollower::new(client, None).with_base_delay(Duration::from_millis(5));
    assert_eq!(follower.next().await.unwrap_err().status(), Some(403));
}
