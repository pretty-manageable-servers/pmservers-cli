use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::Result;

/// A token in a list. The secret is not in this type: the API shows it one time, on create.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: String,
    pub name: String,
    /// `pms_` and the next 4 characters, to tell tokens apart.
    pub prefix: String,
    pub created_at: String,
    /// `null` means the token never expires.
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
}

/// The answer to create. `token` is the full secret. The API does not return it again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedApiToken {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApiToken {
    pub name: String,
    /// `null` means the token never expires. The API allows 1 to 365.
    pub expires_in_days: Option<u32>,
}

impl Client {
    pub async fn list_tokens(&self, project: &str) -> Result<Vec<ApiToken>> {
        Ok(self
            .get::<Items<ApiToken>>(&["v1", "projects", project, "tokens"])
            .await?
            .items)
    }

    /// The answer has the secret in `token`. Save it. The API does not return it again.
    pub async fn create_token(
        &self,
        project: &str,
        name: &str,
        expires_in_days: Option<u32>,
    ) -> Result<CreatedApiToken> {
        let body = CreateApiToken {
            name: name.to_owned(),
            expires_in_days,
        };
        self.post(&["v1", "projects", project, "tokens"], &body)
            .await
    }

    pub async fn revoke_token(&self, project: &str, token_id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "tokens", token_id])
            .await
    }
}
