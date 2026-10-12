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
            .env("PMS_SKIP_KEYCHAIN_MOVE", "1")
            .env("PMS_API_URL", api_url)
            .env_remove("PMS_PROJECT")
            .env_remove("PMS_OUTPUT")
            .env_remove("PMS_TOKEN")
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

pub const PRJ: &str = "prj_aaaaaaaaaaaa";

/// A directory with scripts that stand in for programs. Put it first in PATH.
#[cfg(unix)]
pub fn fake_bin(dir: &Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn container_json(status: &str) -> serde_json::Value {
    json!({"id": "ctr_abc123def456", "name": "api", "type": "worker", "image": "ghcr.io/o/app:1",
           "size": "pms-c-250m-512", "runtime": "gvisor", "vcpu": 0.25, "memory_mib": 512,
           "registry_credential_id": null, "env_names": ["A", "B"], "ports": [8080], "internal_host": "api", "status": status,
           "status_message": null, "created_at": "2026-10-10T10:00:00+00:00",
           "updated_at": "2026-10-10T10:00:00+00:00"})
}
