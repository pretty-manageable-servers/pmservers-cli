use anyhow::Result;

use crate::cli::{CatalogArgs, CatalogKind};
use crate::ctx::Ctx;
use crate::output::print_table;

pub async fn show(ctx: &Ctx, args: CatalogArgs) -> Result<()> {
    let catalog = ctx.client()?.catalog().await?;
    ctx.print(&catalog, |c| {
        let all = matches!(args.what, CatalogKind::All);
        if all {
            println!("Regions: {}\n", c.regions.join(", "));
        }
        if all || matches!(args.what, CatalogKind::Sizes) {
            println!("VM sizes");
            print_table(
                &["id", "vcpu", "memory"],
                c.vm_sizes
                    .iter()
                    .map(|s| {
                        vec![
                            s.id.clone(),
                            s.vcpu.to_string(),
                            format!("{} GiB", s.memory_gib),
                        ]
                    })
                    .collect(),
            );
            for (title, sizes) in [
                ("\nContainer sizes", &c.container_sizes),
                ("\nDatabase sizes", &c.database_sizes),
            ] {
                println!("{title}");
                print_table(
                    &["id", "vcpu", "memory"],
                    sizes
                        .iter()
                        .map(|s| {
                            vec![
                                s.id.clone(),
                                s.vcpu.to_string(),
                                format!("{} MiB", s.memory_mib),
                            ]
                        })
                        .collect(),
                );
            }
        }
        if all || matches!(args.what, CatalogKind::Images) {
            println!("\nVM images");
            print_table(
                &["id", "name", "version"],
                c.vm_images
                    .iter()
                    .map(|i| vec![i.id.clone(), i.name.clone(), i.version.clone()])
                    .collect(),
            );
        }
        if all || matches!(args.what, CatalogKind::Models) {
            println!("\nAI models");
            print_table(
                &["id", "type", "description"],
                c.ai_models
                    .iter()
                    .map(|m| vec![m.id.clone(), m.kind.clone(), m.description.clone()])
                    .collect(),
            );
        }
    });
    Ok(())
}
