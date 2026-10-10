use anyhow::{bail, Result};
use pms_client::core::{Meter, Project};

use crate::cli::ProjectCmd;
use crate::config::Config;
use crate::ctx::Ctx;
use crate::output::{print_kv, print_table, ts};

pub async fn run(ctx: &Ctx, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::List => list(ctx).await,
        ProjectCmd::Create { name, use_it } => create(ctx, &name, use_it).await,
        ProjectCmd::Show => show(ctx).await,
        ProjectCmd::Rename { name } => rename(ctx, &name).await,
        ProjectCmd::Delete => delete(ctx).await,
        ProjectCmd::Use { project } => use_project(ctx, &project).await,
        ProjectCmd::Usage => usage(ctx).await,
        ProjectCmd::Leave => leave(ctx).await,
    }
}

fn details(p: &Project) {
    print_kv(vec![
        ("ID", p.id.clone()),
        ("Name", p.name.clone()),
        ("Region", p.region.clone()),
        ("Role", p.role.clone()),
        ("Created", ts(&p.created_at)),
    ]);
}

async fn list(ctx: &Ctx) -> Result<()> {
    let projects = ctx.client()?.projects().await?;
    let default = Config::load()?.project;
    ctx.print(&projects, |projects| {
        print_table(
            &["id", "name", "region", "role", "default", "created"],
            projects
                .iter()
                .map(|p| {
                    let is_default = default.as_deref() == Some(p.id.as_str());
                    vec![
                        p.id.clone(),
                        p.name.clone(),
                        p.region.clone(),
                        p.role.clone(),
                        if is_default {
                            "*".into()
                        } else {
                            String::new()
                        },
                        ts(&p.created_at),
                    ]
                })
                .collect(),
        )
    });
    Ok(())
}

async fn create(ctx: &Ctx, name: &str, use_it: bool) -> Result<()> {
    let p = ctx.client()?.create_project(name).await?;
    if use_it {
        save_default(&p.id)?;
    }
    ctx.print(&p, details);
    Ok(())
}

async fn show(ctx: &Ctx) -> Result<()> {
    let id = ctx.project_id().await?;
    let p = ctx.client()?.project(&id).await?;
    ctx.print(&p, details);
    Ok(())
}

async fn rename(ctx: &Ctx, name: &str) -> Result<()> {
    let id = ctx.project_id().await?;
    let p = ctx.client()?.rename_project(&id, name).await?;
    ctx.print(&p, details);
    Ok(())
}

async fn delete(ctx: &Ctx) -> Result<()> {
    let id = ctx.project_id().await?;
    let client = ctx.client()?;
    let p = client.project(&id).await?;
    ctx.confirm(&format!("Delete project {} ({})?", p.name, p.id))?;
    client.delete_project(&id).await?;
    let mut config = Config::load()?;
    if config.project.as_deref() == Some(id.as_str()) {
        config.project = None;
        config.save()?;
    }
    ctx.done(&format!("Deleting project {}.", p.name));
    Ok(())
}

fn save_default(id: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.project = Some(id.to_owned());
    config.save()
}

async fn use_project(ctx: &Ctx, project: &str) -> Result<()> {
    let id = ctx.resolve_project(project).await?;
    // Check that it exists and that we can open it.
    let p = ctx.client()?.project(&id).await?;
    save_default(&p.id)?;
    ctx.done(&format!("Default project: {} ({}).", p.name, p.id));
    Ok(())
}

fn num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

async fn usage(ctx: &Ctx) -> Result<()> {
    let id = ctx.project_id().await?;
    let u = ctx.client()?.usage(&id).await?;
    ctx.print(&u, |u| {
        let row = |name: &str, m: &Meter, unit: &str| {
            vec![
                name.to_owned(),
                format!("{}{unit}", num(m.used)),
                format!("{}{unit}", num(m.limit)),
            ]
        };
        print_table(
            &["resource", "used", "limit"],
            vec![
                row("vCPU", &u.vcpu, ""),
                row("Memory", &u.memory_gib, " GiB"),
                row("VM storage", &u.storage_gb, " GB"),
                vec![
                    "AI spend (month)".into(),
                    format!("${}", num(u.ai_spend_usd.used)),
                    format!("${}", num(u.ai_spend_usd.limit)),
                ],
            ],
        )
    });
    Ok(())
}

async fn leave(ctx: &Ctx) -> Result<()> {
    let id = ctx.project_id().await?;
    let client = ctx.client()?;
    let p = client.project(&id).await?;
    if p.role == "owner" {
        bail!("You own this project. Delete it with `pms project delete` instead.");
    }
    ctx.confirm(&format!("Leave project {} ({})?", p.name, p.id))?;
    let me = client.me().await?;
    client.remove_member(&id, &me.id).await?;
    ctx.done(&format!("You left project {}.", p.name));
    Ok(())
}
