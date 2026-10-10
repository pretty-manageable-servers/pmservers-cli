use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub plan: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSize {
    pub id: String,
    pub vcpu: u32,
    pub memory_gib: u32,
}

/// Container and database sizes have the same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeSize {
    pub id: String,
    pub vcpu: f64,
    pub memory_mib: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmImage {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub vm_sizes: Vec<VmSize>,
    #[serde(default)]
    pub container_sizes: Vec<ComputeSize>,
    #[serde(default)]
    pub database_sizes: Vec<ComputeSize>,
    #[serde(default)]
    pub vm_images: Vec<VmImage>,
    #[serde(default)]
    pub ai_models: Vec<AiModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub region: String,
    /// `owner` or `member`.
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
    /// Only in the answer to an invite: true if the email got a Supabase invite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invited: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meter {
    pub used: f64,
    pub limit: f64,
    #[serde(default)]
    pub series: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub vcpu: Meter,
    pub memory_gib: Meter,
    pub storage_gb: Meter,
    pub ai_spend_usd: Meter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Items<T> {
    pub items: Vec<T>,
}

impl Client {
    pub async fn me(&self) -> Result<Me> {
        self.get(&["v1", "me"]).await
    }

    pub async fn catalog(&self) -> Result<Catalog> {
        self.get(&["v1", "catalog"]).await
    }
}

impl Client {
    pub async fn projects(&self) -> Result<Vec<Project>> {
        Ok(self.get::<Items<Project>>(&["v1", "projects"]).await?.items)
    }

    pub async fn create_project(&self, name: &str) -> Result<Project> {
        self.post(&["v1", "projects"], &serde_json::json!({ "name": name }))
            .await
    }

    pub async fn project(&self, id: &str) -> Result<Project> {
        self.get(&["v1", "projects", id]).await
    }

    pub async fn rename_project(&self, id: &str, name: &str) -> Result<Project> {
        self.patch(
            &["v1", "projects", id],
            &serde_json::json!({ "name": name }),
        )
        .await
    }

    /// The owner only. The API refuses while the project still has resources.
    pub async fn delete_project(&self, id: &str) -> Result<()> {
        self.delete(&["v1", "projects", id]).await
    }

    pub async fn usage(&self, project: &str) -> Result<Usage> {
        self.get(&["v1", "projects", project, "usage"]).await
    }

    pub async fn members(&self, project: &str) -> Result<Vec<Member>> {
        Ok(self
            .get::<Items<Member>>(&["v1", "projects", project, "members"])
            .await?
            .items)
    }

    pub async fn invite_member(&self, project: &str, email: &str) -> Result<Member> {
        self.post(
            &["v1", "projects", project, "members"],
            &serde_json::json!({ "email": email }),
        )
        .await
    }

    /// The owner removes a member, or a member removes themselves to leave.
    pub async fn remove_member(&self, project: &str, user_id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "members", user_id])
            .await
    }

    pub async fn ssh_keys(&self) -> Result<Vec<SshKey>> {
        Ok(self.get::<Items<SshKey>>(&["v1", "ssh-keys"]).await?.items)
    }

    pub async fn add_ssh_key(&self, name: &str, public_key: &str) -> Result<SshKey> {
        self.post(
            &["v1", "ssh-keys"],
            &serde_json::json!({ "name": name, "public_key": public_key }),
        )
        .await
    }

    pub async fn delete_ssh_key(&self, id: &str) -> Result<()> {
        self.delete(&["v1", "ssh-keys", id]).await
    }
}
