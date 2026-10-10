#![allow(dead_code)]
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use serde_json::json;
use tempfile::TempDir;

pub struct Env {
    pub dir: TempDir,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl Env {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().unwrap(),
        }
    }

    pub fn config_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("config")
    }

    pub fn credentials(&self) -> std::path::PathBuf {
        self.config_dir().join("credentials.json")
    }

    /// A saved session. `expires_in` can be negative.
    pub fn sign_in(&self, supabase_url: &str, expires_in: i64) {
        std::fs::create_dir_all(self.config_dir()).unwrap();
        std::fs::write(
            self.credentials(),
            json!({
                "access_token": "access-1",
                "refresh_token": "refresh-1",
                "expires_at": now() + expires_in,
                "user_id": "5b1c0e2a-8d41-4f3e-9a77-1e0c3d2b6f90",
                "email": "a@b.co",
                "provider": "email",
                "supabase_url": supabase_url,
            })
            .to_string(),
        )
        .unwrap();
    }

    pub fn cmd(&self, api_url: &str) -> Command {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("pms");
        cmd.env("PMS_CONFIG_DIR", self.config_dir())
            .env("PMS_DATA_DIR", self.dir.path().join("data"))
            .env("PMS_NO_KEYRING", "1")
            .env("PMS_API_URL", api_url)
            .env_remove("PMS_PROJECT")
            .env_remove("PMS_OUTPUT")
            .env_remove("PMS_SUPABASE_URL")
            .env_remove("PMS_SUPABASE_ANON_KEY");
        cmd
    }
}

pub fn read(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

pub fn project_json(id: &str, name: &str, role: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "region": "ca-tor-1", "role": role,
           "created_at": "2026-10-09T10:00:00+00:00"})
}
