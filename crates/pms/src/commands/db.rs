use std::time::Duration;

use anyhow::{anyhow, Result};
use pms_client::databases::{CreateDatabase, Database};
use pms_client::Client;

use crate::cli::{DbCmd, DbConnectArgs, VectorCmd, WaitArgs};
use crate::ctx::Ctx;
use crate::output::{print_kv, print_table, status_text, ts, warn_one_time};
use crate::wait::{self, Snapshot};
use crate::{cloudflared, ssh};

pub async fn run(ctx: &Ctx, cmd: DbCmd) -> Result<()> {
    match cmd {
        DbCmd::List => list(ctx).await,
        DbCmd::Create {
            name,
            size,
            vector,
            wait,
        } => create(ctx, name, size, vector, &wait).await,
        DbCmd::Show { database } => show(ctx, &database).await,
        DbCmd::Delete { database, wait } => delete(ctx, &database, &wait).await,
        DbCmd::Vector(VectorCmd::On { database }) => vector(ctx, &database, true).await,
        DbCmd::Vector(VectorCmd::Off { database }) => vector(ctx, &database, false).await,
        DbCmd::ResetPassword { database } => reset_password(ctx, &database).await,
        DbCmd::Connect(a) => connect(ctx, a).await,
    }
}

async fn find(client: &Client, project: &str, wanted: &str) -> Result<Database> {
    if wanted.starts_with("db_") {
        return Ok(client.database(project, wanted).await?);
    }
    client
        .databases(project)
        .await?
        .into_iter()
        .find(|d| d.name == wanted)
        .ok_or_else(|| anyhow!("No database named {wanted:?}. Run `pms db list`."))
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn used(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "-".into(), mib)
}

fn details(d: &Database) {
    let mut rows = vec![
        ("ID", d.id.clone()),
        ("Name", d.name.clone()),
        ("Status", status_text(&d.status, &d.status_message)),
        ("Engine", d.engine.clone()),
        ("Size", d.size.clone()),
        ("Vector", if d.vector { "on".into() } else { "off".into() }),
        ("Host", d.host.clone()),
        ("Port", d.port.to_string()),
        ("Database", d.database.clone()),
        ("User", d.user.clone()),
        (
            "Storage",
            format!(
                "{} used of {} (all databases of the account)",
                used(d.used_bytes),
                mib(d.storage_limit_bytes)
            ),
        ),
        ("Created", ts(&d.created_at)),
    ];
    if let Some(p) = &d.password {
        rows.push(("Password", p.clone()));
    }
    if let Some(u) = &d.connection_url {
        rows.push(("Connection URL", u.clone()));
    }
    print_kv(rows);
}

/// Print a database that has a secret. The warning goes first, so that it is not missed.
fn print_with_secret(ctx: &Ctx, d: &Database) {
    if !ctx.json() {
        warn_one_time("password");
    }
    ctx.print(d, details);
}

async fn list(ctx: &Ctx) -> Result<()> {
    let project = ctx.project_id().await?;
    let list = ctx.client()?.databases(&project).await?;
    ctx.print(&list, |list| {
        print_table(
            &[
                "id", "name", "status", "size", "vector", "storage", "created",
            ],
            list.iter()
                .map(|d| {
                    vec![
                        d.id.clone(),
                        d.name.clone(),
                        status_text(&d.status, &d.status_message),
                        d.size.clone(),
                        if d.vector { "on".into() } else { "off".into() },
                        format!("{} / {}", used(d.used_bytes), mib(d.storage_limit_bytes)),
                        ts(&d.created_at),
                    ]
                })
                .collect(),
        )
    });
    Ok(())
}

async fn show(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let d = find(&ctx.client()?, &project, wanted).await?;
    ctx.print(&d, details);
    Ok(())
}

fn timeout(w: &WaitArgs) -> Duration {
    Duration::from_secs(w.timeout)
}

fn snapshot(d: Database) -> Snapshot<Database> {
    Snapshot {
        name: d.name.clone(),
        status: d.status.clone(),
        message: d.status_message.clone(),
        value: d,
    }
}

async fn wait_running(client: &Client, project: &str, id: &str, w: &WaitArgs) -> Result<Database> {
    wait::for_status(client, id, timeout(w), "running", false, || async {
        Ok(snapshot(client.database(project, id).await?))
    })
    .await
}

async fn create(
    ctx: &Ctx,
    name: String,
    size: Option<String>,
    vector: bool,
    w: &WaitArgs,
) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let created = client
        .create_database(&project, &CreateDatabase { name, size, vector })
        .await?;
    if !w.wait {
        print_with_secret(ctx, &created);
        return Ok(());
    }
    // In table mode, show the password before the long wait, so that Ctrl-C does not lose it.
    if !ctx.json() {
        print_with_secret(ctx, &created);
    }
    match wait_running(&client, &project, &created.id, w).await {
        Ok(mut ready) => {
            if ctx.json() {
                ready.password = created.password;
                ready.connection_url = created.connection_url;
                ctx.print(&ready, details);
            } else {
                ctx.done(&format!("Database {} is running.", ready.name));
            }
            Ok(())
        }
        Err(e) => {
            if ctx.json() {
                warn_one_time("password");
                eprintln!("Password: {}", created.password.unwrap_or_default());
            }
            Err(e)
        }
    }
}

async fn delete(ctx: &Ctx, wanted: &str, w: &WaitArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let d = find(&client, &project, wanted).await?;
    ctx.confirm(&format!(
        "Delete database {} ({})? There are no backups. The data is gone.",
        d.name, d.id
    ))?;
    client.delete_database(&project, &d.id).await?;
    if w.wait {
        wait::for_gone(&client, &d.id, timeout(w), || async {
            match client.database(&project, &d.id).await {
                Err(e) if e.is_not_found() => Ok(None),
                Err(e) => Err(e.into()),
                Ok(v) => Ok(Some(snapshot(v).forget_value())),
            }
        })
        .await?;
    }
    ctx.done(&format!(
        "{} database {}.",
        if w.wait { "Deleted" } else { "Deleting" },
        d.name
    ));
    Ok(())
}

async fn vector(ctx: &Ctx, wanted: &str, on: bool) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let d = find(&client, &project, wanted).await?;
    let updated = client.set_database_vector(&project, &d.id, on).await?;
    ctx.print(&updated, details);
    Ok(())
}

async fn reset_password(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let d = find(&client, &project, wanted).await?;
    ctx.confirm(&format!(
        "Make a new password for database {}? Apps that use the old one must be changed.",
        d.name
    ))?;
    let updated = client.reset_database_password(&project, &d.id).await?;
    print_with_secret(ctx, &updated);
    Ok(())
}

/// Fail with a clear message if the local port is taken. Only localhost is used.
fn check_port_free(port: u16) -> Result<()> {
    std::net::TcpListener::bind(("127.0.0.1", port))
        .map(drop)
        .map_err(|e| anyhow!("Local port {port} is not free ({e}). Use --port to pick another."))
}

/// Wait until ssh listens on the local port. ssh does this after it has logged in.
async fn wait_listening(child: &mut tokio::process::Child, port: u16) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(anyhow!(
                "ssh stopped before the tunnel was ready ({status}). Check that your key is saved \
                 (`pms ssh-key list`) and pass it with -i if ssh does not find it."
            ));
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Err(anyhow!("The tunnel was not ready after 2 minutes."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn connect(ctx: &Ctx, a: DbConnectArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let db = find(&client, &project, &a.database).await?;
    // Check the database and the port first. Do not offer a download for a database that we cannot reach.
    ssh::database_args(&db, "cloudflared", a.identity.as_deref(), a.port)?;
    check_port_free(a.port)?;
    let cf = cloudflared::ensure(ctx).await?;
    let args = ssh::database_args(&db, &cf.to_string_lossy(), a.identity.as_deref(), a.port)?;
    let mut child = tokio::process::Command::new("ssh")
        .args(&args)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("`ssh` was not found. Install an OpenSSH client.")
            }
            _ => anyhow!("cannot run ssh: {e}"),
        })?;
    // Ctrl-C reaches ssh too, because it is in the same process group. Stop it here as well.
    tokio::select! {
        ready = wait_listening(&mut child, a.port) => ready?,
        _ = tokio::signal::ctrl_c() => return stop(&mut child).await,
    }
    eprintln!(
        "Listening on localhost:{}. Replace <password> with the database password (`pms db reset-password` makes a new one). Press Ctrl-C to stop.",
        a.port
    );
    println!(
        "postgres://{}:<password>@localhost:{}/{}",
        db.user, a.port, db.database
    );
    tokio::select! {
        status = child.wait() => {
            let status = status?;
            Err(anyhow!("ssh stopped ({status}). The tunnel is closed."))
        }
        _ = tokio::signal::ctrl_c() => stop(&mut child).await,
    }
}

async fn stop(child: &mut tokio::process::Child) -> Result<()> {
    let _ = child.kill().await;
    eprintln!("Stopped.");
    Ok(())
}
