use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::Result;
use crate::vms::PowerAction;

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
}

/// Fields that are `None` are not sent and stay as they are. `env` replaces all variables.
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
    pub runtime: Option<String>,
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
