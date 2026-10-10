use anyhow::Result;
use pms_client::containers::Access;

use crate::cli::AccessCmd;
use crate::ctx::Ctx;
use crate::output::{opt, print_kv, ts_opt};

fn details(a: &Access) {
    print_kv(vec![
        ("Access", a.access.clone()),
        ("Runtime", a.runtime.clone()),
        ("Container space", a.vcluster.clone()),
        ("Reason", opt(&a.reason)),
        ("Note", opt(&a.note)),
        ("Requested", ts_opt(&a.requested_at)),
        ("Decided", ts_opt(&a.decided_at)),
    ]);
    match a.access.as_str() {
        "none" | "rejected" => {
            println!("\nAsk for access with: pms access request \"<what you want to run>\"")
        }
        "pending" => println!("\nYour request is waiting for review."),
        _ => {}
    }
}

pub async fn run(ctx: &Ctx, cmd: AccessCmd) -> Result<()> {
    let client = ctx.client()?;
    let a = match cmd {
        AccessCmd::Show => client.access().await?,
        AccessCmd::Request { reason } => client.request_access(&reason).await?,
    };
    ctx.print(&a, details);
    Ok(())
}
