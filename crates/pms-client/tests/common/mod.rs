#![allow(dead_code)]
use std::sync::Arc;

use pms_client::{Client, StaticToken};
use wiremock::MockServer;

pub async fn setup() -> (MockServer, Client) {
    let server = MockServer::start().await;
    let client = Client::new(&server.uri(), Arc::new(StaticToken("tok".into()))).unwrap();
    (server, client)
}
