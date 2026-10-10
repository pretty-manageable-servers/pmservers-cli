use anyhow::Result;
use pms_client::events::{Event, EventFollower};

use crate::cli::EventsArgs;
use crate::ctx::Ctx;
use crate::output::{print_table, ts};

fn row(e: &Event) -> Vec<String> {
    vec![
        ts(&e.time),
        e.level.clone(),
        e.resource_name.clone(),
        e.message.clone(),
    ]
}

pub async fn run(ctx: &Ctx, args: EventsArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let mut events = client.events(&project).await?;
    // The API answers newest first. Show the oldest first, so the newest is at the bottom.
    events.reverse();
    let newest = events.last().map(|e| e.id.clone());
    if !args.follow {
        ctx.print(&events, |events| {
            print_table(
                &["time", "level", "resource", "message"],
                events.iter().map(row).collect(),
            )
        });
        return Ok(());
    }
    if ctx.json() {
        for e in &events {
            println!("{}", serde_json::to_string(e)?);
        }
    } else {
        for e in &events {
            println!("{}", row(e).join("  "));
        }
    }
    // The stream has the events of all projects of the user. Keep this project's.
    let mut follower = EventFollower::new(client, newest);
    loop {
        let e = follower.next().await?;
        if e.project_id.as_deref().is_some_and(|p| p != project) {
            continue;
        }
        if ctx.json() {
            println!("{}", serde_json::to_string(&e)?);
        } else {
            println!("{}", row(&e).join("  "));
        }
    }
}
