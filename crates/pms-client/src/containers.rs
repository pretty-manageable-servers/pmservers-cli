use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::Result;

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

/// What the account may do with containers. `access` is none, pending, approved or rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Access {
    pub access: String,
    /// gvisor or native.
    pub runtime: String,
    /// none, creating, ready, failed or deleting.
    pub vcluster: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub image: String,
    pub size: String,
    pub runtime: String,
    #[serde(default)]
    pub vcpu: Option<f64>,
    #[serde(default)]
    pub memory_mib: Option<u64>,
    #[serde(default)]
    pub registry_credential_id: Option<String>,
    /// The values are write-only. The API returns the names.
    #[serde(default)]
    pub env_names: Vec<String>,
    /// Private TCP ports. Only containers in the same project can connect.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// The name other containers in the same project use, as `host:port`. Null if no ports.
    #[serde(default)]
    pub internal_host: Option<String>,
    /// A managed Cloudflare Tunnel connector attached to this container, if enabled.
    #[serde(default)]
    pub tunnel: Option<TunnelConnection>,
    /// provisioning, running, stopped, failed, deleting or unknown.
    pub status: String,
    #[serde(default)]
    pub status_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateContainer {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub image: String,
    pub size: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_credential_id: Option<String>,
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConnection {
    pub port: u16,
    pub origin: String,
    pub status: String,
    #[serde(default)]
    pub observed_at: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Fields that are `None` are not sent and stay as they are. `env` replaces all variables.
/// `env_set` and `env_unset` change only the named variables. Do not send `env` with them.
/// `registry_credential_id: Some(None)` removes the credential.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateContainer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_credential_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_set: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_unset: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// `Some(vec![])` removes all ports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_token: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub detach_tunnel: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// The most ports for one container.
pub const MAX_PORTS: usize = 5;

/// The same checks as the API: 1 to 5 ports are allowed, no repeats. Port 0 is not valid.
pub fn check_ports(ports: &[u16]) -> std::result::Result<(), String> {
    if ports.len() > MAX_PORTS {
        return Err(format!("A container can have {MAX_PORTS} ports at most."));
    }
    if ports.contains(&0) {
        return Err("A port must be from 1 to 65535.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    if let Some(p) = ports.iter().find(|p| !seen.insert(**p)) {
        return Err(format!("Port {p} is in the list two times."));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logs {
    pub lines: Vec<LogLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCredential {
    pub id: String,
    pub name: String,
    pub host: String,
    pub username: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateRegistryCredential {
    pub name: String,
    pub host: String,
    pub username: String,
    pub token: String,
}

impl Client {
    pub async fn access(&self) -> Result<Access> {
        self.get(&["v1", "account", "access"]).await
    }

    pub async fn request_access(&self, reason: &str) -> Result<Access> {
        self.post(
            &["v1", "account", "access", "request"],
            &serde_json::json!({ "reason": reason }),
        )
        .await
    }

    /// A 403 means the project owner has no container access. The message says which case.
    pub async fn containers(&self, project: &str) -> Result<Vec<Container>> {
        Ok(self
            .get::<Items<Container>>(&["v1", "projects", project, "containers"])
            .await?
            .items)
    }

    pub async fn create_container(
        &self,
        project: &str,
        body: &CreateContainer,
    ) -> Result<Container> {
        self.post(&["v1", "projects", project, "containers"], body)
            .await
    }

    pub async fn container(&self, project: &str, id: &str) -> Result<Container> {
        self.get(&["v1", "projects", project, "containers", id])
            .await
    }

    pub async fn update_container(
        &self,
        project: &str,
        id: &str,
        body: &UpdateContainer,
    ) -> Result<Container> {
        self.patch(&["v1", "projects", project, "containers", id], body)
            .await
    }

    pub async fn delete_container(&self, project: &str, id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "containers", id])
            .await
    }

    pub async fn container_action(
        &self,
        project: &str,
        id: &str,
        action: PowerAction,
    ) -> Result<Container> {
        self.post(
            &["v1", "projects", project, "containers", id, "actions"],
            &serde_json::json!({ "action": action.as_str() }),
        )
        .await
    }

    /// The last `tail` lines of the newest pod. `previous`: the run before the last restart.
    pub async fn container_logs(
        &self,
        project: &str,
        id: &str,
        tail: u32,
        previous: bool,
    ) -> Result<Vec<LogLine>> {
        let mut query = vec![("tail", tail.to_string())];
        if previous {
            query.push(("previous", "true".into()));
        }
        Ok(self
            .get_query::<Logs>(
                &["v1", "projects", project, "containers", id, "logs"],
                &query,
            )
            .await?
            .lines)
    }

    pub async fn registry_credentials(&self) -> Result<Vec<RegistryCredential>> {
        Ok(self
            .get::<Items<RegistryCredential>>(&["v1", "registry-credentials"])
            .await?
            .items)
    }

    pub async fn add_registry_credential(
        &self,
        body: &CreateRegistryCredential,
    ) -> Result<RegistryCredential> {
        self.post(&["v1", "registry-credentials"], body).await
    }

    pub async fn delete_registry_credential(&self, id: &str) -> Result<()> {
        self.delete(&["v1", "registry-credentials", id]).await
    }
}

#[cfg(test)]
mod tests {
    use super::check_ports;

    #[test]
    fn ports_are_checked_like_the_api() {
        assert!(check_ports(&[]).is_ok());
        assert!(check_ports(&[80, 8080, 9000, 5432, 6379]).is_ok());
        assert!(check_ports(&[1, 2, 3, 4, 5, 6]).is_err());
        assert!(check_ports(&[0]).is_err());
        assert!(check_ports(&[80, 80]).is_err());
    }
}
