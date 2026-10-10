use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub time: String,
    /// info, success, warning or error.
    pub level: String,
    pub resource_id: String,
    pub resource_name: String,
    pub message: String,
    /// Only on the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Ticket {
    ticket: String,
}

impl Client {
    /// The newest 50 events of a project, newest first.
    pub async fn events(&self, project: &str) -> Result<Vec<Event>> {
        Ok(self
            .get::<Items<Event>>(&["v1", "projects", project, "events"])
            .await?
            .items)
    }

    pub async fn events_ticket(&self) -> Result<String> {
        Ok(self
            .post_empty::<Ticket>(&["v1", "events", "ticket"])
            .await?
            .ticket)
    }

    /// Open one SSE connection. The ticket lasts 60 seconds, so get one for each connect.
    pub async fn open_event_stream(&self, last_event_id: Option<&str>) -> Result<EventStream> {
        let ticket = self.events_ticket().await?;
        let mut url = self.url(&["v1", "events", "stream"]);
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("ticket", &ticket);
            if let Some(id) = last_event_id {
                q.append_pair("last_event_id", id);
            }
        }
        let resp = self
            .http
            .get(url)
            .header("accept", "text/event-stream")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Api {
                status: resp.status().as_u16(),
                code: "stream".into(),
                message: format!("The event stream answered {}.", resp.status()),
            });
        }
        Ok(EventStream::new(Box::pin(resp.bytes_stream())))
    }
}

type ByteStream = Pin<Box<dyn Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send>>;

/// One SSE connection.
pub struct EventStream {
    body: ByteStream,
    parser: SseParser,
}

impl EventStream {
    fn new(body: ByteStream) -> Self {
        Self {
            body,
            parser: SseParser::default(),
        }
    }

    /// `Ok(None)` when the server closed the stream.
    pub async fn next(&mut self) -> Result<Option<Event>> {
        loop {
            if let Some(event) = self.parser.pop() {
                return Ok(Some(event));
            }
            match self.body.next().await {
                Some(chunk) => self.parser.push(&chunk?),
                None => return Ok(None),
            }
        }
    }
}

/// Minimal SSE parser: `id:`, `event:` and `data:` lines, comments, blank line ends an event.
#[derive(Default)]
struct SseParser {
    buf: Vec<u8>,
    id: Option<String>,
    kind: Option<String>,
    data: Vec<String>,
    ready: std::collections::VecDeque<Event>,
}

impl SseParser {
    fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.line(&String::from_utf8_lossy(&line));
        }
    }

    fn line(&mut self, line: &str) {
        if line.is_empty() {
            self.dispatch();
        } else if line.starts_with(':') {
            // A comment, for example the server ping.
        } else {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "id" => self.id = Some(value.to_owned()),
                "event" => self.kind = Some(value.to_owned()),
                "data" => self.data.push(value.to_owned()),
                _ => {}
            }
        }
    }

    fn dispatch(&mut self) {
        let data = std::mem::take(&mut self.data).join("\n");
        let kind = self.kind.take();
        let id = self.id.clone();
        if data.is_empty() || kind.as_deref().is_some_and(|k| k != "event") {
            return;
        }
        // An event that we cannot read is skipped. The stream goes on.
        if let Ok(mut event) = serde_json::from_str::<Event>(&data) {
            if let Some(id) = id {
                event.id = id;
            }
            self.ready.push_back(event);
        }
    }

    fn pop(&mut self) -> Option<Event> {
        self.ready.pop_front()
    }
}

/// An event stream that reconnects by itself, with `last_event_id`, and backs off after a failure.
pub struct EventFollower {
    client: Client,
    last_id: Option<String>,
    stream: Option<EventStream>,
    failures: u32,
    base_delay: Duration,
}

impl EventFollower {
    pub fn new(client: Client, last_event_id: Option<String>) -> Self {
        Self {
            client,
            last_id: last_event_id,
            stream: None,
            failures: 0,
            base_delay: Duration::from_secs(1),
        }
    }

    pub fn with_base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    pub fn last_event_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// The next event. Network errors are retried. A refused sign-in is returned.
    pub async fn next(&mut self) -> Result<Event> {
        loop {
            if self.stream.is_none() {
                if self.failures > 0 {
                    let exp = self.failures.min(5) - 1;
                    let delay = (self.base_delay * 2u32.pow(exp)).min(Duration::from_secs(30));
                    tokio::time::sleep(delay).await;
                }
                match self.client.open_event_stream(self.last_id.as_deref()).await {
                    Ok(s) => self.stream = Some(s),
                    Err(e) if is_final(&e) => return Err(e),
                    Err(_) => {
                        self.failures += 1;
                        continue;
                    }
                }
            }
            let stream = self.stream.as_mut().expect("opened above");
            match stream.next().await {
                Ok(Some(event)) => {
                    self.failures = 0;
                    self.last_id = Some(event.id.clone());
                    return Ok(event);
                }
                Ok(None) | Err(_) => {
                    self.stream = None;
                    self.failures += 1;
                }
            }
        }
    }
}

fn is_final(e: &Error) -> bool {
    matches!(
        e,
        Error::Auth(_)
            | Error::Api {
                status: 401 | 403,
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_json(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","time":"2026-10-10T10:00:00+00:00","level":"info","resource_id":"vm_1","resource_name":"web","message":"Creating VM","project_id":"prj_1"}}"#
        )
    }

    #[test]
    fn parses_events_across_chunks_and_skips_pings() {
        let mut p = SseParser::default();
        let text = format!(
            ": connected\n\nid: evt_1\nevent: event\ndata: {}\n\n: ping\n\n",
            event_json("evt_1")
        );
        for chunk in text.as_bytes().chunks(7) {
            p.push(chunk);
        }
        let e = p.pop().unwrap();
        assert_eq!(e.id, "evt_1");
        assert_eq!(e.project_id.as_deref(), Some("prj_1"));
        assert!(p.pop().is_none());
    }

    #[test]
    fn handles_crlf_and_skips_other_event_kinds_and_bad_json() {
        let mut p = SseParser::default();
        p.push(b"event: other\r\ndata: {}\r\n\r\ndata: not json\r\n\r\n");
        assert!(p.pop().is_none());
        p.push(format!("event: event\r\ndata: {}\r\n\r\n", event_json("evt_2")).as_bytes());
        assert_eq!(p.pop().unwrap().id, "evt_2");
    }
}
