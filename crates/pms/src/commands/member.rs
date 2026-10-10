use anyhow::{bail, Result};

use crate::cli::MemberCmd;
use crate::ctx::Ctx;
use crate::output::{print_table, ts};

pub async fn run(ctx: &Ctx, cmd: MemberCmd) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    match cmd {
        MemberCmd::List => {
            let members = client.members(&project).await?;
            ctx.print(&members, |members| {
                print_table(
                    &["user id", "email", "role", "added"],
                    members
                        .iter()
                        .map(|m| {
                            vec![
                                m.user_id.clone(),
                                m.email.clone(),
                                m.role.clone(),
                                ts(&m.created_at),
                            ]
                        })
                        .collect(),
                )
            });
        }
        MemberCmd::Invite { email } => {
            let m = client.invite_member(&project, &email).await?;
            let what = if m.invited == Some(true) {
                "Invited"
            } else {
                "Added"
            };
            if ctx.json() {
                crate::output::print_json(&m);
            } else {
                println!("{what} {}.", m.email);
            }
        }
        MemberCmd::Remove { member } => {
            let members = client.members(&project).await?;
            let found = members
                .iter()
                .find(|m| m.user_id == member || m.email.eq_ignore_ascii_case(&member));
            let Some(found) = found else {
                bail!("No member {member:?} in this project. Run `pms member list`.");
            };
            ctx.confirm(&format!("Remove {} from the project?", found.email))?;
            client.remove_member(&project, &found.user_id).await?;
            ctx.done(&format!("Removed {}.", found.email));
        }
    }
    Ok(())
}
