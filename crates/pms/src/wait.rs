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
