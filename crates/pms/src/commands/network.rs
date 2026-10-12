use std::io::IsTerminal;
use std::path::Path;

use anyhow::{bail, Result};
use pms_client::connectors::{AvailableTarget, Connector, PutConnector, TargetInput};

use crate::cli::{CloudflaredCmd, ExposeArgs, NetworkCmd, TailscaleCmd, UnexposeArgs, Via};
use crate::ctx::Ctx;
use crate::output::{opt, print_table};
use crate::token_input::read_token;

/// The two connector kinds. `show` prints them in this order.
const KINDS: [&str; 2] = ["tailscale", "cloudflared"];

pub async fn run(ctx: &Ctx, cmd: NetworkCmd) -> Result<()> {
    match cmd {
        NetworkCmd::Show => show(ctx).await,
        NetworkCmd::Tailscale(TailscaleCmd::Enable { auth_key_file }) => {
            enable(
                ctx,
                "tailscale",
                "Tailscale auth key",
                auth_key_file.as_deref(),
            )
            .await
        }
        NetworkCmd::Tailscale(TailscaleCmd::Disable) => disable(ctx, "tailscale").await,
        NetworkCmd::Cloudflared(CloudflaredCmd::Enable { token_file }) => {
            enable(
                ctx,
                "cloudflared",
                "cloudflared connector token",
                token_file.as_deref(),
            )
            .await
        }
        NetworkCmd::Cloudflared(CloudflaredCmd::Disable) => disable(ctx, "cloudflared").await,
        NetworkCmd::Expose(a) => expose(ctx, a).await,
        NetworkCmd::Unexpose(a) => unexpose(ctx, a).await,
    }
}

/// The token from a private file, or from standard input. A token is never a command-line value.
fn token_from(file: Option<&Path>, what: &str) -> Result<String> {
    if file.is_none() && std::io::stdin().is_terminal() {
        eprintln!("Paste the {what}, then press Ctrl-D.");
    }
    let token = read_token(file, file.is_none())?.unwrap_or_default();
    if token.is_empty() {
        bail!("The {what} is empty.");
    }
    Ok(token)
}

async fn enable(ctx: &Ctx, kind: &str, what: &str, file: Option<&Path>) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let token = token_from(file, what)?;
    // A token makes the connector, or replaces its token. The targets stay as they are.
    let connector = client
        .put_connector(
            &project,
            kind,
            &PutConnector {
                token: Some(token),
                targets: None,
            },
        )
        .await?;
    ctx.print(&connector, |c| {
        print_status(std::slice::from_ref(c), false);
        print_targets(std::slice::from_ref(c));
    });
    Ok(())
}

async fn disable(ctx: &Ctx, kind: &str) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    ctx.confirm(&format!(
        "Turn off the {kind} connector? Its pod goes away and its targets stop being reachable."
    ))?;
    client.delete_connector(&project, kind).await?;
    ctx.done(&format!("Turning off the {kind} connector."));
    Ok(())
}

async fn show(ctx: &Ctx) -> Result<()> {
    let project = ctx.project_id().await?;
    let list = ctx.client()?.connectors(&project).await?;
    ctx.print(&list, |list| {
        print_status(list, true);
        print_targets(list);
    });
    Ok(())
}

/// The kinds and their status. `show_missing` also lists a connector that is off.
fn print_status(list: &[Connector], show_missing: bool) {
    let mut rows = Vec::new();
    for kind in KINDS {
        match list.iter().find(|c| c.kind == kind) {
            Some(c) => rows.push(vec![c.kind.clone(), c.status.clone(), opt(&c.message)]),
            None if show_missing => rows.push(vec![kind.into(), "off".into(), "-".into()]),
            None => {}
        }
    }
    print_table(&["kind", "status", "message"], rows);
}

/// Every target of the connectors, with its address.
fn print_targets(list: &[Connector]) {
    let rows: Vec<Vec<String>> = list
        .iter()
        .flat_map(|c| {
            c.targets.iter().map(move |t| {
                vec![
                    c.kind.clone(),
                    t.kind.clone(),
                    t.name.clone(),
                    t.port.to_string(),
                    t.external_port
                        .map_or_else(|| "-".into(), |p| p.to_string()),
                    opt(&t.address),
                ]
            })
        })
        .collect();
    if rows.is_empty() {
        println!("No targets. Add one with `pms network expose <name>`.");
        return;
    }
    print_table(
        &[
            "connector",
            "kind",
            "name",
            "port",
            "external port",
            "address",
        ],
        rows,
    );
}

/// The container or database that a name or ID means. Refuse none and more than one.
fn find_target<'a>(wanted: &str, available: &'a [AvailableTarget]) -> Result<&'a AvailableTarget> {
    let matches: Vec<&AvailableTarget> = available
        .iter()
        .filter(|t| t.name == wanted || t.id == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(*one),
        [] => bail!(
            "No container or database named {wanted:?} in this project. Run `pms container list` or `pms db list`."
        ),
        _ => bail!("More than one container or database is named {wanted:?}. Use the ID."),
    }
}

fn ports_text(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The port to expose: the port that was given, or the only port of the container or database.
fn expose_port(target: &AvailableTarget, port: Option<u16>) -> Result<u16> {
    match (port, target.ports.as_slice()) {
        (Some(p), ports) if ports.contains(&p) => Ok(p),
        (Some(p), _) => bail!(
            "Port {p} is not a port of {}. Its ports: {}.",
            target.name,
            ports_text(&target.ports)
        ),
        (None, [only]) => Ok(*only),
        (None, []) => bail!(
            "{} has no ports to expose. Add a port with `pms container edit`.",
            target.name
        ),
        (None, _) => bail!(
            "{} has more than one port. Use --port with one of: {}.",
            target.name,
            ports_text(&target.ports)
        ),
    }
}

/// The connectors that `--via` selects. Each one must exist. With `both`, the ones that exist.
fn chosen_connectors(via: Via, connectors: &[Connector]) -> Result<Vec<&Connector>> {
    let chosen: Vec<&Connector> = KINDS
        .iter()
        .filter(|kind| match via {
            Via::Tailscale => **kind == "tailscale",
            Via::Cloudflared => **kind == "cloudflared",
            Via::Both => true,
        })
        .filter_map(|kind| connectors.iter().find(|c| c.kind == *kind))
        .collect();
    if !chosen.is_empty() {
        return Ok(chosen);
    }
    let message = match via {
        Via::Tailscale => "The Tailscale connector is off. Turn it on with `pms network tailscale enable`.",
        Via::Cloudflared => "The cloudflared connector is off. Turn it on with `pms network cloudflared enable`.",
        Via::Both => "Neither connector is on. Turn one on with `pms network tailscale enable` or `pms network cloudflared enable`.",
    };
    bail!("{message}")
}

/// `--external-port` is for Tailscale only.
fn check_external_port(chosen: &[&Connector], external_port: Option<u16>) -> Result<()> {
    if external_port.is_some() && !chosen.iter().any(|c| c.kind == "tailscale") {
        bail!("--external-port is for Tailscale only. Use --via tailscale.");
    }
    Ok(())
}

fn same_resource(t: &TargetInput, resource: &AvailableTarget) -> bool {
    t.kind == resource.kind && t.id == resource.id
}

/// The targets of one connector after `expose`. A target that is already there is changed in place.
/// Tailscale: the external port is the one given, else the old one, else the target port.
/// cloudflared: no external port.
fn expose_targets(
    connector_kind: &str,
    current: &[TargetInput],
    resource: &AvailableTarget,
    port: u16,
    external_port: Option<u16>,
) -> Vec<TargetInput> {
    let old_external = current
        .iter()
        .find(|t| same_resource(t, resource) && t.port == port)
        .and_then(|t| t.external_port);
    let new = TargetInput {
        kind: resource.kind.clone(),
        id: resource.id.clone(),
        port,
        external_port: match connector_kind {
            "tailscale" => Some(external_port.or(old_external).unwrap_or(port)),
            _ => None,
        },
    };
    let mut next: Vec<TargetInput> = Vec::new();
    let mut placed = false;
    for t in current {
        if same_resource(t, resource) && t.port == port {
            if !placed {
                next.push(new.clone());
                placed = true;
            }
        } else {
            next.push(t.clone());
        }
    }
    if !placed {
        next.push(new);
    }
    next
}

/// The targets of one connector after `unexpose`. Without a port, every port of the resource goes.
fn unexpose_targets(
    current: &[TargetInput],
    resource: &AvailableTarget,
    port: Option<u16>,
) -> Vec<TargetInput> {
    current
        .iter()
        .filter(|t| !(same_resource(t, resource) && port.is_none_or(|p| t.port == p)))
        .cloned()
        .collect()
}

async fn expose(ctx: &Ctx, a: ExposeArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let available = client.available_targets(&project).await?;
    let resource = find_target(&a.name, &available)?;
    let port = expose_port(resource, a.port)?;
    let connectors = client.connectors(&project).await?;
    let chosen = chosen_connectors(a.via, &connectors)?;
    check_external_port(&chosen, a.external_port)?;
    let mut touched = Vec::new();
    for connector in chosen {
        let current: Vec<TargetInput> = connector.targets.iter().map(TargetInput::from).collect();
        let next = expose_targets(&connector.kind, &current, resource, port, a.external_port);
        if next == current {
            touched.push(connector.clone());
            continue;
        }
        let updated = client
            .put_connector(
                &project,
                &connector.kind,
                &PutConnector {
                    token: None,
                    targets: Some(next),
                },
            )
            .await?;
        touched.push(updated);
    }
    ctx.print(&touched, |list| print_targets(list));
    Ok(())
}

async fn unexpose(ctx: &Ctx, a: UnexposeArgs) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    let available = client.available_targets(&project).await?;
    let resource = find_target(&a.name, &available)?;
    let connectors = client.connectors(&project).await?;
    let chosen = chosen_connectors(a.via, &connectors)?;
    let mut touched = Vec::new();
    for connector in chosen {
        let current: Vec<TargetInput> = connector.targets.iter().map(TargetInput::from).collect();
        let next = unexpose_targets(&current, resource, a.port);
        if next == current {
            touched.push(connector.clone());
            continue;
        }
        let updated = client
            .put_connector(
                &project,
                &connector.kind,
                &PutConnector {
                    token: None,
                    targets: Some(next),
                },
            )
            .await?;
        touched.push(updated);
    }
    ctx.print(&touched, |list| print_targets(list));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(kind: &str, id: &str, name: &str, ports: &[u16]) -> AvailableTarget {
        AvailableTarget {
            kind: kind.into(),
            id: id.into(),
            name: name.into(),
            ports: ports.to_vec(),
        }
    }

    fn input(kind: &str, id: &str, port: u16, external_port: Option<u16>) -> TargetInput {
        TargetInput {
            kind: kind.into(),
            id: id.into(),
            port,
            external_port,
        }
    }

    fn connector(kind: &str, targets: Vec<TargetInput>) -> Connector {
        Connector {
            id: "ncn_abc".into(),
            kind: kind.into(),
            status: "connected".into(),
            message: None,
            observed_at: None,
            targets: targets
                .into_iter()
                .map(|t| pms_client::connectors::ConnectorTarget {
                    kind: t.kind,
                    id: t.id,
                    name: "x".into(),
                    port: t.port,
                    external_port: t.external_port,
                    address: None,
                })
                .collect(),
            created_at: "2026-10-11T10:00:00+00:00".into(),
            updated_at: "2026-10-11T10:00:00+00:00".into(),
        }
    }

    #[test]
    fn a_name_or_an_id_finds_one_target() {
        let all = [
            resource("container", "ctr_1", "api", &[3000]),
            resource("database", "db_1", "main", &[5432]),
        ];
        assert_eq!(find_target("api", &all).unwrap().id, "ctr_1");
        assert_eq!(find_target("db_1", &all).unwrap().name, "main");
    }

    #[test]
    fn a_missing_name_is_an_error() {
        let all = [resource("container", "ctr_1", "api", &[3000])];
        let err = find_target("web", &all).unwrap_err().to_string();
        assert!(
            err.contains("No container or database named \"web\""),
            "{err}"
        );
    }

    #[test]
    fn a_name_shared_by_a_container_and_a_database_is_ambiguous() {
        let all = [
            resource("container", "ctr_1", "app", &[3000]),
            resource("database", "db_1", "app", &[5432]),
        ];
        let err = find_target("app", &all).unwrap_err().to_string();
        assert!(err.contains("More than one"), "{err}");
        assert_eq!(find_target("db_1", &all).unwrap().kind, "database");
    }

    #[test]
    fn the_only_port_is_the_default() {
        let api = resource("container", "ctr_1", "api", &[3000]);
        assert_eq!(expose_port(&api, None).unwrap(), 3000);
        assert_eq!(expose_port(&api, Some(3000)).unwrap(), 3000);
        assert!(expose_port(&api, Some(8080)).is_err());
    }

    #[test]
    fn more_than_one_port_needs_port_and_no_ports_is_an_error() {
        let many = resource("container", "ctr_1", "api", &[3000, 8080]);
        let err = expose_port(&many, None).unwrap_err().to_string();
        assert!(err.contains("more than one port"), "{err}");
        assert_eq!(expose_port(&many, Some(8080)).unwrap(), 8080);
        let none = resource("container", "ctr_2", "job", &[]);
        assert!(expose_port(&none, None).is_err());
    }

    #[test]
    fn expose_adds_a_tailscale_target_with_the_port_as_external_port() {
        let api = resource("container", "ctr_1", "api", &[3000]);
        let next = expose_targets("tailscale", &[], &api, 3000, None);
        assert_eq!(next, vec![input("container", "ctr_1", 3000, Some(3000))]);
    }

    #[test]
    fn expose_keeps_the_old_external_port_unless_one_is_given() {
        let api = resource("container", "ctr_1", "api", &[3000]);
        let current = vec![input("container", "ctr_1", 3000, Some(4000))];
        assert_eq!(
            expose_targets("tailscale", &current, &api, 3000, None),
            current
        );
        let changed = expose_targets("tailscale", &current, &api, 3000, Some(5000));
        assert_eq!(changed, vec![input("container", "ctr_1", 3000, Some(5000))]);
    }

    #[test]
    fn expose_on_cloudflared_has_no_external_port() {
        let db = resource("database", "db_1", "main", &[5432]);
        let next = expose_targets("cloudflared", &[], &db, 5432, None);
        assert_eq!(next, vec![input("database", "db_1", 5432, None)]);
    }

    #[test]
    fn expose_keeps_the_other_targets_and_their_order() {
        let api = resource("container", "ctr_1", "api", &[3000]);
        let current = vec![
            input("database", "db_1", 5432, Some(5432)),
            input("container", "ctr_1", 3000, Some(3000)),
            input("container", "ctr_2", 80, Some(80)),
        ];
        // Already there: the list does not change at all.
        assert_eq!(
            expose_targets("tailscale", &current, &api, 3000, None),
            current
        );
        let added = expose_targets("tailscale", &current[..1], &api, 3000, None);
        assert_eq!(added[0], current[0]);
        assert_eq!(added[1], input("container", "ctr_1", 3000, Some(3000)));
    }

    #[test]
    fn unexpose_removes_one_port_or_all_ports_of_the_resource() {
        let api = resource("container", "ctr_1", "api", &[3000, 8080]);
        let current = vec![
            input("container", "ctr_1", 3000, Some(3000)),
            input("container", "ctr_1", 8080, Some(8080)),
            input("database", "db_1", 5432, Some(5432)),
        ];
        assert_eq!(
            unexpose_targets(&current, &api, Some(8080)),
            vec![current[0].clone(), current[2].clone()]
        );
        assert_eq!(
            unexpose_targets(&current, &api, None),
            vec![current[2].clone()]
        );
        // Not exposed: the list is the same, so no PUT is sent.
        let other = resource("container", "ctr_9", "web", &[80]);
        assert_eq!(unexpose_targets(&current, &other, None), current);
    }

    #[test]
    fn via_selects_the_connectors_that_exist() {
        let both = [
            connector("tailscale", vec![]),
            connector("cloudflared", vec![]),
        ];
        assert_eq!(chosen_connectors(Via::Both, &both).unwrap().len(), 2);

        let only_cf = [connector("cloudflared", vec![])];
        let chosen = chosen_connectors(Via::Both, &only_cf).unwrap();
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].kind, "cloudflared");
        assert!(chosen_connectors(Via::Tailscale, &only_cf).is_err());

        assert!(chosen_connectors(Via::Both, &[]).is_err());
    }

    #[test]
    fn external_port_needs_tailscale() {
        let only_cf = [connector("cloudflared", vec![])];
        let chosen = chosen_connectors(Via::Both, &only_cf).unwrap();
        assert!(check_external_port(&chosen, Some(5000)).is_err());
        assert!(check_external_port(&chosen, None).is_ok());
    }
}
