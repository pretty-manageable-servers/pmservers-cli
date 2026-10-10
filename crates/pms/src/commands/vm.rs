use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use pms_client::vms::{CreateVm, PowerAction, Vm};
use pms_client::Client;

use crate::cli::{CreateVmArgs, SshConfigArgs, VmCmd, VmSshArgs, WaitArgs};
use crate::ctx::Ctx;
use crate::output::{opt, print_kv, print_table, status_text, ts};
use crate::wait::{self, Step};
use crate::{cloudflared, console, keygen, ssh};

pub async fn run(ctx: &Ctx, cmd: VmCmd) -> Result<()> {
    match cmd {
        VmCmd::List => list(ctx).await,
        VmCmd::Create(a) => create(ctx, a).await,
        VmCmd::Show { vm } => show(ctx, &vm).await,
        VmCmd::Delete { vm, wait } => delete(ctx, &vm, &wait).await,
        VmCmd::Start { vm, wait } => power(ctx, &vm, PowerAction::Start, &wait).await,
        VmCmd::Stop { vm, wait } => power(ctx, &vm, PowerAction::Stop, &wait).await,
        VmCmd::Restart { vm, wait } => power(ctx, &vm, PowerAction::Restart, &wait).await,
        VmCmd::Metrics { vm } => metrics(ctx, &vm).await,
        VmCmd::Console { vm } => open_console(ctx, &vm).await,
        VmCmd::Ssh(a) => connect(ctx, a).await,
    }
}

/// By ID (`vm_...`) or by name.
async fn find_vm(client: &Client, project: &str, wanted: &str) -> Result<Vm> {
    if wanted.starts_with("vm_") {
        return Ok(client.vm(project, wanted).await?);
    }
    let vms = client.vms(project).await?;
    vms.into_iter()
        .find(|v| v.name == wanted)
        .ok_or_else(|| anyhow!("No VM named {wanted:?}. Run `pms vm list`."))
}

fn details(vm: &Vm) {
    print_kv(vec![
        ("ID", vm.id.clone()),
        ("Name", vm.name.clone()),
        ("Status", status_text(&vm.status, &vm.status_message)),
        ("Size", vm.size.clone()),
        ("Image", vm.image.clone()),
        ("Disk", format!("{} GB", vm.disk_gb)),
        ("Private IP", opt(&vm.private_ip)),
        ("SSH user", vm.ssh_user.clone()),
        ("Jump host", vm.ssh_host.clone()),
        ("Created", ts(&vm.created_at)),
    ]);
}

async fn list(ctx: &Ctx) -> Result<()> {
    let project = ctx.project_id().await?;
    let vms = ctx.client()?.vms(&project).await?;
    ctx.print(&vms, |vms| {
        print_table(
            &[
                "id", "name", "status", "size", "image", "disk", "ip", "created",
            ],
            vms.iter()
                .map(|v| {
                    vec![
                        v.id.clone(),
                        v.name.clone(),
                        status_text(&v.status, &v.status_message),
                        v.size.clone(),
                        v.image.clone(),
                        format!("{} GB", v.disk_gb),
                        opt(&v.private_ip),
                        ts(&v.created_at),
                    ]
                })
                .collect(),
        )
    });
    Ok(())
}

async fn show(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let vm = find_vm(&ctx.client()?, &project, wanted).await?;
    ctx.print(&vm, details);
    Ok(())
}

fn timeout(w: &WaitArgs) -> Duration {
    Duration::from_secs(w.timeout)
}

/// Wait for a VM to reach `target`. A failed VM is an error.
async fn wait_for_status(
    client: &Client,
    project: &str,
    id: &str,
    target: &str,
    w: &WaitArgs,
) -> Result<Vm> {
    wait::until(client, id, timeout(w), || async {
        let vm = client.vm(project, id).await?;
        Ok(if vm.status == target {
            Step::Done(vm)
        } else if vm.status == "failed" {
            Step::Failed(format!(
                "VM {} failed: {}",
                vm.name,
                vm.status_message.as_deref().unwrap_or("no reason given")
            ))
        } else {
            Step::Pending(format!(
                "{}: {}",
                vm.name,
                status_text(&vm.status, &vm.status_message)
            ))
        })
    })
    .await
}

async fn create(ctx: &Ctx, a: CreateVmArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let mut generated: Option<(std::path::PathBuf, keygen::KeyPair)> = None;
    let public_key = if let Some(wanted) = &a.ssh_key {
        let keys = client.ssh_keys().await?;
        keys.into_iter()
            .find(|k| &k.id == wanted || &k.name == wanted)
            .ok_or_else(|| anyhow!("No saved SSH key {wanted:?}. Run `pms ssh-key list`."))?
            .public_key
    } else if let Some(file) = &a.public_key_file {
        std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?
            .trim()
            .to_owned()
    } else {
        let path = match &a.key_out {
            Some(p) => p.clone(),
            None => keygen::default_key_path(&a.name)?,
        };
        keygen::check_free(&path)?;
        let pair = keygen::generate(&format!("pms-{}", a.name))?;
        let key = pair.public_openssh.clone();
        generated = Some((path, pair));
        key
    };

    // Write the private key before the call, so that a VM is never made without its key on disk.
    if let Some((path, pair)) = &generated {
        keygen::write_pair(path, pair)?;
    }
    let body = CreateVm {
        name: a.name.clone(),
        size: a.size,
        image: a.image,
        disk_gb: a.disk_gb,
        ssh_public_key: public_key,
    };
    let mut vm = match client.create_vm(&project, &body).await {
        Ok(vm) => vm,
        Err(e) => {
            if let Some((path, _)) = &generated {
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_file(format!("{}.pub", path.display()));
            }
            return Err(e.into());
        }
    };
    if let Some((path, _)) = &generated {
        eprintln!(
            "Private key saved in {}. It is not stored anywhere else.\nConnect with: pms vm ssh {} -i {}",
            path.display(),
            vm.name,
            path.display()
        );
    }
    if a.wait.wait {
        vm = wait_for_status(&client, &project, &vm.id, "running", &a.wait).await?;
    }
    ctx.print(&vm, details);
    Ok(())
}

async fn delete(ctx: &Ctx, wanted: &str, w: &WaitArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let vm = find_vm(&client, &project, wanted).await?;
    ctx.confirm(&format!(
        "Delete VM {} ({})? The disk is deleted too.",
        vm.name, vm.id
    ))?;
    client.delete_vm(&project, &vm.id).await?;
    if w.wait {
        wait::until(&client, &vm.id, timeout(w), || async {
            Ok(match client.vm(&project, &vm.id).await {
                Err(e) if e.is_not_found() => Step::Done(()),
                Err(e) => return Err(e.into()),
                Ok(v) => Step::Pending(format!(
                    "{}: {}",
                    v.name,
                    status_text(&v.status, &v.status_message)
                )),
            })
        })
        .await?;
    }
    ctx.done(&format!(
        "{} VM {}.",
        if w.wait { "Deleted" } else { "Deleting" },
        vm.name
    ));
    Ok(())
}

async fn power(ctx: &Ctx, wanted: &str, action: PowerAction, w: &WaitArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let vm = find_vm(&client, &project, wanted).await?;
    let mut vm = client.vm_action(&project, &vm.id, action).await?;
    if w.wait {
        vm = match action {
            PowerAction::Start => wait_for_status(&client, &project, &vm.id, "running", w).await?,
            PowerAction::Stop => wait_for_status(&client, &project, &vm.id, "stopped", w).await?,
            PowerAction::Restart => wait_restart(&client, &project, &vm, w).await?,
        };
    }
    ctx.print(&vm, details);
    Ok(())
}

/// Right after the call the VM can still say `running`. Wait until it left that state, or a
/// grace time has passed, and then until it runs again.
async fn wait_restart(client: &Client, project: &str, vm: &Vm, w: &WaitArgs) -> Result<Vm> {
    let started = Instant::now();
    let left_running = std::cell::Cell::new(vm.status != "running");
    wait::until(client, &vm.id, timeout(w), || {
        let left_running = &left_running;
        async move {
            let now = client.vm(project, &vm.id).await?;
            if now.status != "running" {
                left_running.set(true);
            }
            Ok(
                if now.status == "running"
                    && (left_running.get() || started.elapsed() > Duration::from_secs(20))
                {
                    Step::Done(now)
                } else if now.status == "failed" {
                    Step::Failed(format!(
                        "VM {} failed: {}",
                        now.name,
                        now.status_message.as_deref().unwrap_or("no reason given")
                    ))
                } else {
                    Step::Pending(format!(
                        "{}: {}",
                        now.name,
                        status_text(&now.status, &now.status_message)
                    ))
                },
            )
        }
    })
    .await
}

async fn metrics(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let vm = find_vm(&client, &project, wanted).await?;
    let m = client.vm_metrics(&project, &vm.id).await?;
    ctx.print(&m, |m| {
        if m.cpu.is_empty() {
            println!("No metrics yet.");
        } else {
            for point in &m.cpu {
                println!("{point}");
            }
        }
    });
    Ok(())
}

async fn open_console(ctx: &Ctx, wanted: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let vm = find_vm(&client, &project, wanted).await?;
    if vm.status != "running" {
        bail!(
            "VM {} is {}. The console needs a running VM.",
            vm.name,
            vm.status
        );
    }
    let ticket = client.console_ticket(&project, &vm.id).await?;
    let url = client.console_ws_url(&ticket)?;
    console::run(&url).await
}

async fn connect(ctx: &Ctx, a: VmSshArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let vm = find_vm(&client, &project, &a.vm).await?;
    // Check the VM first. Do not offer a download for a VM that we cannot reach anyway.
    ssh::build_args(
        &vm,
        "cloudflared",
        a.identity.as_deref(),
        a.user.as_deref(),
        &a.extra,
    )?;
    let cf = cloudflared::ensure(ctx).await?;
    let args = ssh::build_args(
        &vm,
        &cf.to_string_lossy(),
        a.identity.as_deref(),
        a.user.as_deref(),
        &a.extra,
    )?;
    ssh::run(&args)
}

pub async fn ssh_config(ctx: &Ctx, a: SshConfigArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let vms = ctx.client()?.vms(&project).await?;
    let jump = a
        .jump_host
        .or_else(|| {
            vms.iter()
                .find(|v| !v.ssh_host.is_empty())
                .map(|v| v.ssh_host.clone())
        })
        .ok_or_else(|| anyhow!("No jump host known. Pass --jump-host, or make a VM first."))?;
    let cf = cloudflared::locate()?
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cloudflared".into());
    print!("{}", ssh::render_config(&jump, &cf, &vms));
    Ok(())
}
