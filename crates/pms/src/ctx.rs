use std::io::IsTerminal;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, bail, Result};
use pms_client::Client;
use serde::Serialize;

use crate::output::{print_json, Format};
use crate::session::{self, SessionTokens};

pub struct Ctx {
    pub format: Format,
    pub yes: bool,
    pub api_url: String,
    client: OnceLock<Client>,
}

impl Ctx {
    pub fn new(format: Format, yes: bool, api_url: String) -> Self {
        Self {
            format,
            yes,
            api_url,
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
        let session =
            session::load()?.ok_or_else(|| anyhow!("You are not signed in. Run `pms login`."))?;
        let client = Client::new(&self.api_url, Arc::new(SessionTokens::new(session)))?;
        Ok(self.client.get_or_init(|| client).clone())
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
