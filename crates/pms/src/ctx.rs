use std::io::IsTerminal;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, bail, Result};
use pms_client::{Client, StaticToken, TokenSource};
use serde::Serialize;

use crate::config::Config;
use crate::output::{print_json, Format};
use crate::session::{self, SessionTokens};

pub struct Ctx {
    pub format: Format,
    pub yes: bool,
    pub api_url: String,
    pub project: Option<String>,
    client: OnceLock<Client>,
}

impl Ctx {
    pub fn new(format: Format, yes: bool, api_url: String, project: Option<String>) -> Self {
        Self {
            format,
            yes,
            api_url,
            project,
            client: OnceLock::new(),
        }
    }

    pub fn json(&self) -> bool {
        self.format == Format::Json
    }

    pub fn client(&self) -> Result<Client> {
        if let Some(c) = self.client.get() {
            return Ok(c.clone());
        }
        let tokens: Arc<dyn TokenSource> = match session::env_token() {
            Some(token) => Arc::new(StaticToken(token)),
            None => {
                let session = session::load()?
                    .ok_or_else(|| anyhow!("You are not signed in. Run `pms login`."))?;
                Arc::new(SessionTokens::new(session))
            }
        };
        let client = Client::new(&self.api_url, tokens)?;
        Ok(self.client.get_or_init(|| client).clone())
    }

    /// The project ID from `--project` (ID or name), `PMS_PROJECT`, or `pms project use`.
    pub async fn project_id(&self) -> Result<String> {
        let wanted = match &self.project {
            Some(p) => p.clone(),
            None => Config::load()?.project.ok_or_else(|| {
                anyhow!("No project set. Use --project, set PMS_PROJECT, or run `pms project use <project>`.")
            })?,
        };
        self.resolve_project(&wanted).await
    }

    pub async fn resolve_project(&self, wanted: &str) -> Result<String> {
        if wanted.starts_with("prj_") {
            return Ok(wanted.to_owned());
        }
        let projects = self.client()?.projects().await?;
        let mut found = projects.iter().filter(|p| p.name == wanted);
        match (found.next(), found.next()) {
            (Some(p), None) => Ok(p.id.clone()),
            (Some(_), Some(_)) => {
                bail!("More than one project is named {wanted:?}. Use the project ID.")
            }
            _ => bail!("No project named {wanted:?}. Run `pms project list`."),
        }
    }

    /// Ask before a delete. Without a terminal, `--yes` is required.
    pub fn confirm(&self, prompt: &str) -> Result<()> {
        if self.yes {
            return Ok(());
        }
        if !std::io::stdin().is_terminal() {
            bail!("{prompt} Pass --yes to confirm without a terminal.");
        }
        let ok = dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()?;
        if !ok {
            bail!("Cancelled.");
        }
        Ok(())
    }

    /// Print a result for a command that returns no data.
    pub fn done(&self, message: &str) {
        if self.json() {
            print_json(&serde_json::json!({"ok": true, "message": message}));
        } else {
            println!("{message}");
        }
    }

    pub fn print<T: Serialize>(&self, value: &T, table: impl FnOnce(&T)) {
        if self.json() {
            print_json(value);
        } else {
            table(value);
        }
    }
}
