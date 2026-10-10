use std::future::Future;
use std::time::Duration;

use anyhow::{bail, Result};
use pms_client::{events::EventFollower, Client};
use tokio::time::Instant;

pub enum Step<T> {
    Done(T),
    Pending(String),
    Failed(String),
}

/// Poll `check` until it is done. The event stream wakes the loop at once when the resource
/// changes. A slow poll is the fallback, also when the stream is not available.
pub async fn until<T, F, Fut>(
    client: &Client,
    resource_id: &str,
    timeout: Duration,
    mut check: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Step<T>>>,
{
    let deadline = Instant::now() + timeout;
    let mut follower = Some(EventFollower::new(client.clone(), None));
    let mut shown = String::new();
    loop {
        match check().await? {
            Step::Done(v) => return Ok(v),
            Step::Failed(message) => bail!("{message}"),
            Step::Pending(text) => {
                if text != shown {
                    eprintln!("{text}");
                    shown = text;
                }
            }
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "Timed out after {} seconds. The resource is still changing.",
                timeout.as_secs()
            );
        }
        let poll = Duration::from_secs(if follower.is_some() { 10 } else { 3 });
        let wake = (now + poll).min(deadline);
        loop {
            let Some(f) = follower.as_mut() else {
                tokio::time::sleep_until(wake).await;
                break;
            };
            tokio::select! {
                event = f.next() => match event {
                    Ok(e) if e.resource_id == resource_id => break,
                    Ok(_) => {}
                    Err(_) => {
                        follower = None;
                        break;
                    }
                },
                _ = tokio::time::sleep_until(wake) => break,
            }
        }
    }
}

/// One look at a resource, for the wait helpers below.
pub struct Snapshot<T> {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
    pub value: T,
}

impl<T> Snapshot<T> {
    pub fn forget_value(self) -> Snapshot<()> {
        Snapshot {
            name: self.name,
            status: self.status,
            message: self.message,
            value: (),
        }
    }

    fn text(&self) -> String {
        format!(
            "{}: {}",
            self.name,
            crate::output::status_text(&self.status, &self.message)
        )
    }
}

/// Wait until `status` is `target`. A `failed` status is an error. With `restart`, the status
/// must first leave `target`, or 20 seconds must pass: right after the call a restarting
/// resource can still say `running`.
pub async fn for_status<T, F, Fut>(
    client: &Client,
    id: &str,
    timeout: Duration,
    target: &str,
    restart: bool,
    mut fetch: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Snapshot<T>>>,
{
    let started = std::time::Instant::now();
    let left_target = std::cell::Cell::new(!restart);
    until(client, id, timeout, || {
        let fut = fetch();
        let left_target = &left_target;
        async move {
            let snap = fut.await?;
            if snap.status != target {
                left_target.set(true);
            }
            Ok(
                if snap.status == target
                    && (left_target.get() || started.elapsed() > Duration::from_secs(20))
                {
                    Step::Done(snap.value)
                } else if snap.status == "failed" {
                    Step::Failed(format!(
                        "{} failed: {}",
                        snap.name,
                        snap.message.as_deref().unwrap_or("no reason given")
                    ))
                } else {
                    Step::Pending(snap.text())
                },
            )
        }
    })
    .await
}

/// Wait until `fetch` says the resource is gone (`None`).
pub async fn for_gone<F, Fut>(
    client: &Client,
    id: &str,
    timeout: Duration,
    mut fetch: F,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<Snapshot<()>>>>,
{
    until(client, id, timeout, || {
        let fut = fetch();
        async move {
            Ok(match fut.await? {
                None => Step::Done(()),
                Some(snap) => Step::Pending(snap.text()),
            })
        }
    })
    .await
}
