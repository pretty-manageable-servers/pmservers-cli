use serde::{Deserialize, Serialize};
use url::Url;

use crate::client::Client;
use crate::core::Items;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vm {
    pub id: String,
    pub name: String,
    /// provisioning, running, stopped, failed, deleting or pending.
    pub status: String,
    #[serde(default)]
    pub status_message: Option<String>,
    pub size: String,
    pub image: String,
    pub disk_gb: u32,
    #[serde(default)]
    pub private_ip: Option<String>,
    /// The SSH jump host. Empty where there is none (local development).
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default)]
    pub ssh_user: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVm {
    pub name: String,
    pub size: String,
    pub image: String,
    pub disk_gb: u32,
    pub ssh_public_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Start,
    Stop,
    Restart,
}

impl PowerAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PowerAction::Start => "start",
            PowerAction::Stop => "stop",
            PowerAction::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    #[serde(default)]
    pub cpu: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleTicket {
    pub ticket: String,
    /// Path and query of the WebSocket, for example `/v1/projects/p/vms/v/console?ticket=...`.
    pub url: String,
}

impl Client {
    pub async fn vms(&self, project: &str) -> Result<Vec<Vm>> {
        Ok(self
            .get::<Items<Vm>>(&["v1", "projects", project, "vms"])
            .await?
            .items)
    }

    pub async fn create_vm(&self, project: &str, body: &CreateVm) -> Result<Vm> {
        self.post(&["v1", "projects", project, "vms"], body).await
    }

    pub async fn vm(&self, project: &str, id: &str) -> Result<Vm> {
        self.get(&["v1", "projects", project, "vms", id]).await
    }

    pub async fn delete_vm(&self, project: &str, id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "vms", id]).await
    }

    pub async fn vm_action(&self, project: &str, id: &str, action: PowerAction) -> Result<Vm> {
        self.post(
            &["v1", "projects", project, "vms", id, "actions"],
            &serde_json::json!({ "action": action.as_str() }),
        )
        .await
    }

    pub async fn vm_metrics(&self, project: &str, id: &str) -> Result<VmMetrics> {
        self.get(&["v1", "projects", project, "vms", id, "metrics"])
            .await
    }

    pub async fn console_ticket(&self, project: &str, id: &str) -> Result<ConsoleTicket> {
        self.post_empty(&["v1", "projects", project, "vms", id, "console", "ticket"])
            .await
    }

    /// The `ws://` or `wss://` address for a console ticket.
    pub fn console_ws_url(&self, ticket: &ConsoleTicket) -> Result<Url> {
        let mut url = self
            .base
            .join(&ticket.url)
            .map_err(|e| Error::Decode(format!("bad console URL: {e}")))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| Error::Decode("bad console URL scheme".into()))?;
        Ok(url)
    }
}
