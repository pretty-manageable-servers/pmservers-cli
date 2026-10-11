use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use pms_client::containers::{check_ports, Container, CreateContainer, LogLine, UpdateContainer};
use pms_client::vms::PowerAction;
use pms_client::Client;

use crate::cli::{
    ContainerCmd, CreateContainerArgs, EditContainerArgs, EnvArgs, LogsArgs, WaitArgs,
};
use crate::commands::registry;
use crate::ctx::Ctx;
use crate::envfile;
use crate::output::{opt, print_kv, print_table, status_text, ts};
use crate::wait::{self, Snapshot};

pub async fn run(ctx: &Ctx, cmd: ContainerCmd) -> Result<()> {
    match cmd {
        ContainerCmd::List => list(ctx).await,
        ContainerCmd::Create(a) => create(ctx, a).await,
        ContainerCmd::Show { container } => show(ctx, &container).await,
        ContainerCmd::Edit(a) => edit(ctx, a).await,
        ContainerCmd::Delete { container, wait } => delete(ctx, &container, &wait).await,
        ContainerCmd::Start { container, wait } => {
            power(ctx, &container, PowerAction::Start, &wait).await
        }
        ContainerCmd::Stop { container, wait } => {
            power(ctx, &container, PowerAction::Stop, &wait).await
        }
        ContainerCmd::Restart { container, wait } => {
            power(ctx, &container, PowerAction::Restart, &wait).await
        }
        ContainerCmd::Logs(a) => logs(ctx, a).await,
    }
}

async fn find(client: &Client, project: &str, wanted: &str) -> Result<Container> {
    if wanted.starts_with("ctr_") {
        return Ok(client.container(project, wanted).await?);
    }
    client
        .containers(project)
        .await?
        .into_iter()
        .find(|c| c.name == wanted)
        .ok_or_else(|| anyhow!("No container named {wanted:?}. Run `pms container list`."))
}

fn details(c: &Container) {
    print_kv(vec![
        ("ID", c.id.clone()),
        ("Name", c.name.clone()),
        ("Status", status_text(&c.status, &c.status_message)),
        ("Image", c.image.clone()),
        ("Size", c.size.clone()),
        ("Runtime", c.runtime.clone()),
        ("Registry credential", opt(&c.registry_credential_id)),
        (
            "Ports",
            if c.ports.is_empty() {
                "-".into()
            } else {
                c.ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ),
        (
            "Internal address",
            match &c.internal_host {
                Some(h) if !c.ports.is_empty() => c
                    .ports
                    .iter()
                    .map(|p| format!("{h}:{p}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => "-".into(),
            },
        ),
        (
            "Managed tunnel",
            c.tunnel.as_ref().map_or_else(
                || "-".into(),
                |t| {
                    format!(
                        "{} · {} · {}",
                        t.status,
                        t.origin,
                        t.message.as_deref().unwrap_or("")
                    )
                },
            ),
        ),
        (
            "Variables",
            if c.env_names.is_empty() {
                "-".into()
            } else {
                c.env_names.join(", ")
            },
        ),
        ("Created", ts(&c.created_at)),
        ("Updated", ts(&c.updated_at)),
    ]);
}

async fn list(ctx: &Ctx) -> Result<()> {
    let project = ctx.project_id().await?;
    let list = ctx.client()?.containers(&project).await?;
    ctx.print(&list, |list| {
        print_table(
            &[
                "id", "name", "status", "image", "size", "runtime", "created",
            ],
            list.iter()
                .map(|c| {
                    vec![
                        c.id.clone(),
                        c.name.clone(),
                        status_text(&c.status, &c.status_message),
                        c.image.clone(),
                        c.size.clone(),
                        c.runtime.clone(),
                        ts(&c.created_at),
                    ]
                })
                .collect(),
        )
    });
    Ok(())
}

async fn show(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let c = find(&ctx.client()?, &project, wanted).await?;
    ctx.print(&c, details);
    Ok(())
}

fn timeout(w: &WaitArgs) -> Duration {
    Duration::from_secs(w.timeout)
}

fn snapshot(c: Container) -> Snapshot<Container> {
    Snapshot {
        name: c.name.clone(),
        status: c.status.clone(),
        message: c.status_message.clone(),
        value: c,
    }
}

async fn wait_for_status(
    client: &Client,
    project: &str,
    id: &str,
    target: &str,
    w: &WaitArgs,
    restart: bool,
) -> Result<Container> {
    wait::for_status(client, id, timeout(w), target, restart, || async {
        Ok(snapshot(client.container(project, id).await?))
    })
    .await
}

/// A credential by ID or name.
async fn credential_id(client: &Client, wanted: &str) -> Result<String> {
    Ok(registry::find(client, wanted).await?.id)
}

fn env_given(env: &EnvArgs) -> bool {
    !env.env.is_empty() || env.env_file.is_some()
}

fn read_tunnel_token(file: Option<&std::path::Path>, stdin: bool) -> Result<Option<String>> {
    if let Some(path) = file {
        let metadata = std::fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                bail!("{} must be private (chmod 600)", path.display());
            }
        }
        if !metadata.is_file() {
            bail!("{} is not a regular file", path.display());
        }
        let token = std::fs::read_to_string(path)?.trim().to_owned();
        return Ok(Some(token));
    }
    if stdin {
        let mut token = String::new();
        std::io::stdin().read_to_string(&mut token)?;
        return Ok(Some(token.trim().to_owned()));
    }
    Ok(None)
}

async fn create(ctx: &Ctx, a: CreateContainerArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    check_ports(&a.ports).map_err(|e| anyhow!(e))?;
    let env = envfile::collect("--env", &a.env.env, a.env.env_file.as_deref())?;
    let tunnel_token = read_tunnel_token(a.tunnel_token_file.as_deref(), a.tunnel_token_stdin)?;
    if tunnel_token.is_some() != a.tunnel_port.is_some() {
        bail!("--tunnel-port and a token source are required together");
    }
    if let Some(port) = a.tunnel_port {
        if !a.ports.contains(&port) {
            bail!("--tunnel-port must also be listed with --port");
        }
    }
    let registry_credential_id = match &a.registry_credential {
        Some(w) => Some(credential_id(&client, w).await?),
        None => None,
    };
    let mut c = client
        .create_container(
            &project,
            &CreateContainer {
                name: a.name,
                kind: "worker".into(),
                image: a.image,
                size: a.size,
                registry_credential_id,
                env,
                runtime: a.runtime,
                ports: a.ports,
                tunnel_port: a.tunnel_port,
                tunnel_token,
            },
        )
        .await?;
    if a.wait.wait {
        c = wait_for_status(&client, &project, &c.id, "running", &a.wait, false).await?;
    }
    ctx.print(&c, details);
    Ok(())
}

async fn edit(ctx: &Ctx, a: EditContainerArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let current = find(&client, &project, &a.container).await?;
    let mut update = UpdateContainer {
        image: a.image,
        size: a.size,
        runtime: a.runtime,
        ..Default::default()
    };
    if a.clear_ports {
        update.ports = Some(Vec::new());
    } else if !a.ports.is_empty() {
        check_ports(&a.ports).map_err(|e| anyhow!(e))?;
        update.ports = Some(a.ports);
        // The new list replaces the old list.
        eprintln!("Note: these ports replace all ports of the container.");
    }
    update.tunnel_port = a.tunnel_port;
    update.detach_tunnel = a.detach_tunnel;
    update.tunnel_token = read_tunnel_token(a.tunnel_token_file.as_deref(), a.tunnel_token_stdin)?;
    if let Some(port) = update.tunnel_port {
        let target_ports = update.ports.as_deref().unwrap_or(&current.ports);
        if !target_ports.contains(&port) {
            bail!("--tunnel-port must also be listed with --port");
        }
    }
    if update.tunnel_token.is_some() && current.tunnel.is_none() && update.tunnel_port.is_none() {
        bail!("attaching a tunnel needs --tunnel-port");
    }
    if a.no_registry_credential {
        update.registry_credential_id = Some(None);
    } else if let Some(w) = &a.registry_credential {
        update.registry_credential_id = Some(Some(credential_id(&client, w).await?));
    }
    if a.clear_env {
        update.env = Some(BTreeMap::new());
    } else if env_given(&a.env) {
        update.env = Some(envfile::collect(
            "--env",
            &a.env.env,
            a.env.env_file.as_deref(),
        )?);
        // Values are write-only, so --env cannot keep the old values of other variables.
        eprintln!("Note: --env replaces all variables. Use --set-env to change only some.");
    }
    if !a.set_env.is_empty() || a.set_env_file.is_some() {
        update.env_set = Some(envfile::collect(
            "--set-env",
            &a.set_env,
            a.set_env_file.as_deref(),
        )?);
    }
    if !a.unset_env.is_empty() {
        update.env_unset = Some(a.unset_env);
    }
    if update.image.is_none()
        && update.size.is_none()
        && update.runtime.is_none()
        && update.registry_credential_id.is_none()
        && update.env.is_none()
        && update.ports.is_none()
        && update.env_set.is_none()
        && update.env_unset.is_none()
        && update.tunnel_port.is_none()
        && update.tunnel_token.is_none()
        && !update.detach_tunnel
    {
        bail!("Nothing to change. Pass --image, --size, --runtime, --port, --clear-ports, --registry-credential, --env, --clear-env, --set-env, --unset-env, --tunnel-port, a tunnel token source or --detach-tunnel.");
    }
    let mut c = client
        .update_container(&project, &current.id, &update)
        .await?;
    if a.wait.wait {
        c = wait_for_status(&client, &project, &c.id, "running", &a.wait, true).await?;
    }
    ctx.print(&c, details);
    Ok(())
}

async fn delete(ctx: &Ctx, wanted: &str, w: &WaitArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let c = find(&client, &project, wanted).await?;
    ctx.confirm(&format!("Delete container {} ({})?", c.name, c.id))?;
    client.delete_container(&project, &c.id).await?;
    if w.wait {
        wait::for_gone(&client, &c.id, timeout(w), || async {
            match client.container(&project, &c.id).await {
                Err(e) if e.is_not_found() => Ok(None),
                Err(e) => Err(e.into()),
                Ok(v) => Ok(Some(snapshot(v).forget_value())),
            }
        })
        .await?;
    }
    ctx.done(&format!(
        "{} container {}.",
        if w.wait { "Deleted" } else { "Deleting" },
        c.name
    ));
    Ok(())
}

async fn power(ctx: &Ctx, wanted: &str, action: PowerAction, w: &WaitArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let c = find(&client, &project, wanted).await?;
    let mut c = client.container_action(&project, &c.id, action).await?;
    if w.wait {
        let (target, restart) = match action {
            PowerAction::Start => ("running", false),
            PowerAction::Stop => ("stopped", false),
            PowerAction::Restart => ("running", true),
        };
        c = wait_for_status(&client, &project, &c.id, target, w, restart).await?;
    }
    ctx.print(&c, details);
    Ok(())
}

/// The lines after the last one we printed. The API sends a tail window every time.
pub fn new_lines<'a>(last: Option<&LogLine>, lines: &'a [LogLine]) -> &'a [LogLine] {
    match last.and_then(|l| lines.iter().rposition(|x| x == l)) {
        Some(i) => &lines[i + 1..],
        None => lines,
    }
}

fn print_lines(ctx: &Ctx, lines: &[LogLine], ndjson: bool) -> Result<()> {
    for l in lines {
        if ctx.json() && ndjson {
            println!("{}", serde_json::to_string(l)?);
        } else {
            println!("{} {}", l.time, l.message);
        }
    }
    Ok(())
}

async fn logs(ctx: &Ctx, a: LogsArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let c = find(&client, &project, &a.container).await?;
    let lines = client
        .container_logs(&project, &c.id, a.tail, a.previous)
        .await?;
    if !a.follow {
        if ctx.json() {
            crate::output::print_json(&lines);
        } else if lines.is_empty() {
            eprintln!("No log lines.");
        } else {
            print_lines(ctx, &lines, false)?;
        }
        return Ok(());
    }
    print_lines(ctx, &lines, true)?;
    let mut last = lines.last().cloned();
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let lines = client
            .container_logs(&project, &c.id, a.tail, a.previous)
            .await?;
        print_lines(ctx, new_lines(last.as_ref(), &lines), true)?;
        if let Some(l) = lines.last() {
            last = Some(l.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &str, m: &str) -> LogLine {
        LogLine {
            time: t.into(),
            message: m.into(),
        }
    }

    #[test]
    fn only_new_lines_after_the_last_one() {
        let all = [line("1", "a"), line("2", "b"), line("3", "c")];
        assert_eq!(new_lines(None, &all).len(), 3);
        assert_eq!(new_lines(Some(&line("2", "b")), &all), &all[2..]);
        assert!(new_lines(Some(&line("3", "c")), &all).is_empty());
        // The window moved past our last line: print all.
        assert_eq!(new_lines(Some(&line("0", "gone")), &all).len(), 3);
    }

    #[test]
    fn a_repeated_message_with_another_time_is_not_a_duplicate() {
        let all = [line("1", "x"), line("2", "x")];
        assert_eq!(new_lines(Some(&line("1", "x")), &all), &all[1..]);
    }
}
