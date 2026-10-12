use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Error, Result};

/// The answer of the list route. The API uses `connectors`, not `items`.
#[derive(Deserialize)]
struct ConnectorList {
    connectors: Vec<Connector>,
}

/// The answer of the available-targets route.
#[derive(Deserialize)]
struct AvailableTargetList {
    targets: Vec<AvailableTarget>,
}

/// One connector of a project: `tailscale` or `cloudflared`. The token is never returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connector {
    pub id: String,
    pub kind: String,
    /// provisioning, connected, disconnected, stopped or deleting.
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub observed_at: Option<String>,
    pub targets: Vec<ConnectorTarget>,
    pub created_at: String,
    pub updated_at: String,
}

/// A container or database that the connector can reach on one port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorTarget {
    /// container or database.
    pub kind: String,
    pub id: String,
    pub name: String,
    pub port: u16,
    /// Tailscale only. The port on the tailnet name.
    #[serde(default)]
    pub external_port: Option<u16>,
    /// The address the connector serves. Null until it is known.
    #[serde(default)]
    pub address: Option<String>,
}

/// A target in a PUT body. `external_port` is for Tailscale only. Leave it out for cloudflared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetInput {
    pub kind: String,
    pub id: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_port: Option<u16>,
}

impl From<&ConnectorTarget> for TargetInput {
    fn from(target: &ConnectorTarget) -> Self {
        Self {
            kind: target.kind.clone(),
            id: target.id.clone(),
            port: target.port,
            external_port: target.external_port,
        }
    }
}

/// A PUT body. A missing `token` keeps the token. A missing `targets` keeps the targets.
/// The token is needed to make a connector. It is not Debug, so it is never printed.
#[derive(Clone, Serialize)]
pub struct PutConnector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<TargetInput>>,
}

/// A container or database that a connector can reach, with its ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableTarget {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub ports: Vec<u16>,
}

impl Client {
    pub async fn connectors(&self, project: &str) -> Result<Vec<Connector>> {
        Ok(self
            .get::<ConnectorList>(&["v1", "projects", project, "connectors"])
            .await?
            .connectors)
    }

    /// The containers and databases of the project that a connector can reach.
    pub async fn available_targets(&self, project: &str) -> Result<Vec<AvailableTarget>> {
        Ok(self
            .get::<AvailableTargetList>(&[
                "v1",
                "projects",
                project,
                "connectors",
                "available-targets",
            ])
            .await?
            .targets)
    }

    /// Make the connector (needs a token), change its token, or change its targets.
    pub async fn put_connector(
        &self,
        project: &str,
        kind: &str,
        body: &PutConnector,
    ) -> Result<Connector> {
        // The client has no PUT helper yet. This is the same as `patch`, with PUT.
        let value = serde_json::to_value(body).map_err(|e| Error::Invalid(e.to_string()))?;
        let bytes = self
            .send(
                Method::PUT,
                self.url(&["v1", "projects", project, "connectors", kind]),
                Some(value),
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::Decode(e.to_string()))
    }

    /// Turn the connector off. Its pod and its targets go away.
    pub async fn delete_connector(&self, project: &str, kind: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "connectors", kind])
            .await
    }
}
