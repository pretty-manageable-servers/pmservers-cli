use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub size: String,
    #[serde(default)]
    pub vcpu: Option<f64>,
    #[serde(default)]
    pub memory_mib: Option<u64>,
    pub vector: bool,
    /// provisioning, running, read_only, suspended, failed, deleting or unknown.
    pub status: String,
    #[serde(default)]
    pub status_message: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub used_bytes: u64,
    /// The limit is for all databases of the account together.
    pub storage_limit_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
    /// Only in the answer to create and to reset-password. It is shown one time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateDatabase {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    pub vector: bool,
}

impl Client {
    pub async fn databases(&self, project: &str) -> Result<Vec<Database>> {
        Ok(self
            .get::<Items<Database>>(&["v1", "projects", project, "databases"])
            .await?
            .items)
    }

    /// The answer has `password` and `connection_url`. The API does not return them again.
    pub async fn create_database(&self, project: &str, body: &CreateDatabase) -> Result<Database> {
        self.post(&["v1", "projects", project, "databases"], body)
            .await
    }

    pub async fn database(&self, project: &str, id: &str) -> Result<Database> {
        self.get(&["v1", "projects", project, "databases", id])
            .await
    }

    pub async fn set_database_vector(&self, project: &str, id: &str, on: bool) -> Result<Database> {
        self.patch(
            &["v1", "projects", project, "databases", id],
            &serde_json::json!({ "vector": on }),
        )
        .await
    }

    /// The answer has the new `password` and `connection_url`.
    pub async fn reset_database_password(&self, project: &str, id: &str) -> Result<Database> {
        self.post_empty(&["v1", "projects", project, "databases", id, "reset-password"])
            .await
    }

    /// No backups: the data is gone.
    pub async fn delete_database(&self, project: &str, id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "databases", id])
            .await
    }
}
