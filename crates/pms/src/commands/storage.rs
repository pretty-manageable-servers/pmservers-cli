use anyhow::{anyhow, bail, Result};
use pms_client::storage::{
    Bucket, CreateBucket, CreateStorageKey, KeyBuckets, Storage, StorageKey, StorageKeyWithSecret,
};

use crate::cli::{AccountArgs, BucketCmd, CreateStorageKeyArgs, StorageCmd, StorageKeyCmd};
use crate::config::Config;
use crate::ctx::Ctx;
use crate::output::{print_kv, print_table, status_text, ts, warn_one_time};
use crate::session;

pub async fn run(ctx: &Ctx, cmd: StorageCmd) -> Result<()> {
    // The API answers 403 to API tokens on every storage route. Say so before any call.
    if session::env_token().is_some() {
        bail!(
            "Object storage does not accept API tokens. Unset PMS_TOKEN and sign in with `pms login`."
        );
    }
    let result = match cmd {
        StorageCmd::Status(a) => status(ctx, &a).await,
        StorageCmd::Enable(a) => enable(ctx, &a).await,
        StorageCmd::Disable(a) => disable(ctx, &a).await,
        StorageCmd::Bucket(c) => bucket(ctx, c).await,
        StorageCmd::Key(c) => key(ctx, c).await,
    };
    result.map_err(explain)
}

/// The owner ID of the storage account. `--account` gives it. Else the project from `--project`,
/// `PMS_PROJECT` or `pms project use`, like the other commands: the owner of that project is the account.
/// No project at all: `None`, which is the caller's own account.
async fn account(ctx: &Ctx, a: &AccountArgs) -> Result<Option<String>> {
    if let Some(owner) = &a.account {
        if ctx.project_typed {
            bail!("--account and --project cannot be used together. Use one of them.");
        }
        return Ok(Some(owner.clone()));
    }
    let project = match &ctx.project {
        Some(p) => Some(p.clone()),
        None => Config::load()?.project,
    };
    match project {
        None => Ok(None),
        Some(project) => {
            let id = ctx.resolve_project(&project).await?;
            Ok(Some(ctx.client()?.project(&id).await?.owner_id))
        }
    }
}

/// Make the API code `not_available` (the global switch is off) a clear message. Other errors stay as they are.
fn explain(e: anyhow::Error) -> anyhow::Error {
    let off = matches!(
        e.downcast_ref::<pms_client::Error>()
            .and_then(|err| err.code()),
        Some("not_available")
    );
    if off {
        anyhow!(
            "Object storage is not available now. It is switched off for all accounts. Try again later."
        )
    } else {
        e
    }
}

/// Bytes in the unit that fits: B, KiB, MiB or GiB.
fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024f64.powi(3) {
        format!("{:.2} GiB", b / 1024f64.powi(3))
    } else if b >= 1024f64.powi(2) {
        format!("{:.1} MiB", b / 1024f64.powi(2))
    } else if b >= 1024.0 {
        format!("{:.1} KiB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn yes_no(v: bool) -> String {
    if v {
        "yes".into()
    } else {
        "no".into()
    }
}

// --- status, enable, disable ---

async fn status(ctx: &Ctx, a: &AccountArgs) -> Result<()> {
    let s = ctx
        .client()?
        .storage(account(ctx, a).await?.as_deref())
        .await?;
    ctx.print(&s, storage_details);
    Ok(())
}

async fn enable(ctx: &Ctx, a: &AccountArgs) -> Result<()> {
    let s = ctx
        .client()?
        .enable_storage(account(ctx, a).await?.as_deref())
        .await?;
    ctx.print(&s, storage_details);
    Ok(())
}

async fn disable(ctx: &Ctx, a: &AccountArgs) -> Result<()> {
    ctx.confirm(
        "Turn off object storage for this account? Its keys and data are deleted. There are no backups.",
    )?;
    let s = ctx
        .client()?
        .disable_storage(account(ctx, a).await?.as_deref())
        .await?;
    ctx.print(&s, storage_details);
    Ok(())
}

fn storage_details(s: &Storage) {
    let state = s.status.clone().unwrap_or_else(|| "off".into());
    print_kv(vec![
        ("Status", status_text(&state, &s.status_message)),
        ("Endpoint", s.endpoint.clone()),
        ("Region", s.region.clone()),
        (
            "Storage",
            format!(
                "{} used of {} (all buckets of the account)",
                size(s.used_bytes),
                size(s.limit_bytes)
            ),
        ),
        (
            "Buckets",
            format!("{} of {}", s.buckets_count, s.buckets_limit),
        ),
        ("Keys", format!("{} of {}", s.keys_count, s.keys_limit)),
        (
            "Would cost",
            format!("${:.2} per month (not charged yet)", s.would_cost_usd),
        ),
    ]);
}

// --- buckets ---

async fn bucket(ctx: &Ctx, cmd: BucketCmd) -> Result<()> {
    match cmd {
        BucketCmd::List(a) => bucket_list(ctx, &a).await,
        BucketCmd::Create { name, public, acct } => bucket_create(ctx, name, public, &acct).await,
        BucketCmd::Delete { name, acct } => bucket_delete(ctx, &name, &acct).await,
        BucketCmd::Public { name, state, acct } => {
            let b = ctx
                .client()?
                .set_bucket_public(account(ctx, &acct).await?.as_deref(), &name, state.is_on())
                .await?;
            ctx.print(&b, bucket_details);
            Ok(())
        }
    }
}

async fn bucket_list(ctx: &Ctx, a: &AccountArgs) -> Result<()> {
    let list = ctx
        .client()?
        .storage_buckets(account(ctx, a).await?.as_deref())
        .await?;
    ctx.print(&list, |list| {
        print_table(
            &["name", "status", "public", "used", "objects", "created"],
            list.iter()
                .map(|b| {
                    vec![
                        b.name.clone(),
                        status_text(&b.status, &b.status_message),
                        yes_no(b.public_read),
                        size(b.used_bytes),
                        b.object_count.to_string(),
                        ts(&b.created_at),
                    ]
                })
                .collect(),
        )
    });
    Ok(())
}

async fn bucket_create(ctx: &Ctx, name: String, public: bool, a: &AccountArgs) -> Result<()> {
    let body = CreateBucket {
        name,
        public_read: public,
    };
    let b = ctx
        .client()?
        .create_bucket(account(ctx, a).await?.as_deref(), &body)
        .await?;
    ctx.print(&b, bucket_details);
    Ok(())
}

async fn bucket_delete(ctx: &Ctx, name: &str, a: &AccountArgs) -> Result<()> {
    ctx.confirm(&format!(
        "Delete bucket {name}? Only an empty bucket can be deleted. There are no backups."
    ))?;
    ctx.client()?
        .delete_bucket(account(ctx, a).await?.as_deref(), name)
        .await?;
    ctx.done(&format!("Deleting bucket {name}."));
    Ok(())
}

fn bucket_details(b: &Bucket) {
    print_kv(vec![
        ("Name", b.name.clone()),
        ("Status", status_text(&b.status, &b.status_message)),
        ("Public read", yes_no(b.public_read)),
        ("Used", size(b.used_bytes)),
        ("Objects", b.object_count.to_string()),
        ("Created", ts(&b.created_at)),
    ]);
}

// --- access keys ---

async fn key(ctx: &Ctx, cmd: StorageKeyCmd) -> Result<()> {
    match cmd {
        StorageKeyCmd::List(a) => key_list(ctx, &a).await,
        StorageKeyCmd::Create(a) => key_create(ctx, a).await,
        StorageKeyCmd::Reset { key_id, env, acct } => key_reset(ctx, &key_id, env, &acct).await,
        StorageKeyCmd::Delete { key_id, acct } => key_delete(ctx, &key_id, &acct).await,
    }
}

async fn key_list(ctx: &Ctx, a: &AccountArgs) -> Result<()> {
    let list = ctx
        .client()?
        .storage_keys(account(ctx, a).await?.as_deref())
        .await?;
    ctx.print(&list, |list| {
        print_table(
            &["key id", "name", "permission", "buckets", "created"],
            list.iter().map(key_row).collect(),
        )
    });
    Ok(())
}

fn key_row(k: &StorageKey) -> Vec<String> {
    vec![
        k.key_id.clone(),
        k.name.clone(),
        k.permission.clone(),
        k.buckets.describe(),
        ts(&k.created_at),
    ]
}

async fn key_create(ctx: &Ctx, a: CreateStorageKeyArgs) -> Result<()> {
    check_env_format(ctx, a.env)?;
    let body = CreateStorageKey {
        name: a.name,
        permission: permission(a.read_only),
        buckets: buckets(a.buckets),
    };
    let key = ctx
        .client()?
        .create_storage_key(account(ctx, &a.acct).await?.as_deref(), &body)
        .await?;
    print_secret(ctx, &key, a.env);
    Ok(())
}

async fn key_reset(ctx: &Ctx, key_id: &str, env: bool, a: &AccountArgs) -> Result<()> {
    check_env_format(ctx, env)?;
    ctx.confirm(&format!(
        "Make a new key ID and secret for {key_id}? The old key ID and secret stop working. Apps that use them must change."
    ))?;
    let key = ctx
        .client()?
        .reset_storage_key(account(ctx, a).await?.as_deref(), key_id)
        .await?;
    print_secret(ctx, &key, env);
    Ok(())
}

async fn key_delete(ctx: &Ctx, key_id: &str, a: &AccountArgs) -> Result<()> {
    ctx.confirm(&format!(
        "Delete key {key_id}? Apps that use it stop working."
    ))?;
    ctx.client()?
        .delete_storage_key(account(ctx, a).await?.as_deref(), key_id)
        .await?;
    ctx.done(&format!("Deleted key {key_id}."));
    Ok(())
}

/// `--env` prints shell lines, so it cannot be JSON too. Check before the API call: a key must not be made
/// for a command that then fails.
fn check_env_format(ctx: &Ctx, env: bool) -> Result<()> {
    if env && ctx.json() {
        bail!("--env cannot be used with --output json. Use one of them.");
    }
    Ok(())
}

fn permission(read_only: bool) -> String {
    if read_only { "read_only" } else { "read_write" }.into()
}

/// No `--bucket` means all buckets, also the ones made later.
fn buckets(names: Vec<String>) -> KeyBuckets {
    if names.is_empty() {
        KeyBuckets::all()
    } else {
        KeyBuckets::Named(names)
    }
}

/// The secret is shown one time, so the warning goes first. The `--env` lines go to standard output only,
/// so that `eval "$(pms storage key create ... --env)"` works.
fn print_secret(ctx: &Ctx, key: &StorageKeyWithSecret, env: bool) {
    if !ctx.json() {
        warn_one_time("secret access key");
    }
    if env {
        print!("{}", env_lines(key));
        return;
    }
    ctx.print(key, secret_details);
}

fn secret_details(k: &StorageKeyWithSecret) {
    print_kv(vec![
        ("Key ID", k.key_id.clone()),
        ("Name", k.name.clone()),
        ("Permission", k.permission.clone()),
        ("Buckets", k.buckets.describe()),
        ("Endpoint", k.endpoint.clone()),
        ("Region", k.region.clone()),
        ("Secret access key", k.secret_access_key.clone()),
    ]);
}

/// Export lines for the AWS CLI and the AWS SDKs. The first line is a comment, so `eval` still works.
fn env_lines(k: &StorageKeyWithSecret) -> String {
    format!(
        "# Use path style: aws configure set default.s3.addressing_style path\n\
         export AWS_ACCESS_KEY_ID={}\n\
         export AWS_SECRET_ACCESS_KEY={}\n\
         export AWS_ENDPOINT_URL={}\n\
         export AWS_REGION={}\n\
         export AWS_DEFAULT_REGION={}\n",
        shell_quote(&k.key_id),
        shell_quote(&k.secret_access_key),
        shell_quote(&k.endpoint),
        shell_quote(&k.region),
        shell_quote(&k.region),
    )
}

/// Single quotes, so that the shell does not change the value. A `'` becomes `'\''`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> StorageKeyWithSecret {
        StorageKeyWithSecret {
            key_id: "GK0123456789abcdef01234567".into(),
            name: "ci".into(),
            permission: "read_write".into(),
            buckets: KeyBuckets::all(),
            created_at: "2026-10-11T10:00:00+00:00".into(),
            updated_at: "2026-10-11T10:00:00+00:00".into(),
            endpoint: "https://s3.pmservers.org".into(),
            region: "pms".into(),
            secret_access_key: "abc/def+ghi'jkl".into(),
        }
    }

    #[test]
    fn sizes_use_the_unit_that_fits() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(2048), "2.0 KiB");
        assert_eq!(size(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(size(5 * 1024 * 1024 * 1024), "5.00 GiB");
    }

    #[test]
    fn no_bucket_flag_means_all_buckets() {
        assert_eq!(buckets(vec![]), KeyBuckets::All("all".into()));
        assert_eq!(
            buckets(vec!["a".into(), "b".into()]),
            KeyBuckets::Named(vec!["a".into(), "b".into()])
        );
        assert_eq!(permission(true), "read_only");
        assert_eq!(permission(false), "read_write");
    }

    #[test]
    fn env_lines_quote_every_value() {
        let text = env_lines(&sample_key());
        assert!(text.contains("export AWS_ACCESS_KEY_ID='GK0123456789abcdef01234567'\n"));
        assert!(text.contains("export AWS_SECRET_ACCESS_KEY='abc/def+ghi'\\''jkl'\n"));
        assert!(text.contains("export AWS_ENDPOINT_URL='https://s3.pmservers.org'\n"));
        assert!(text.contains("export AWS_DEFAULT_REGION='pms'\n"));
    }

    #[test]
    fn env_and_json_cannot_mix() {
        let ctx = Ctx::new(crate::output::Format::Json, false, String::new(), None);
        assert!(check_env_format(&ctx, true).is_err());
        assert!(check_env_format(&ctx, false).is_ok());
    }
}
