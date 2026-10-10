use std::io::{IsTerminal, Read, Write};

use anyhow::{anyhow, bail, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

/// Ctrl-] leaves the console, as in telnet.
const ESCAPE: u8 = 0x1d;

#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    Escaped,
    Closed(Option<String>),
}

/// The bytes to send, and whether the escape key was in the input.
pub fn split_escape(buf: &[u8]) -> (&[u8], bool) {
    match buf.iter().position(|b| *b == ESCAPE) {
        Some(i) => (&buf[..i], true),
        None => (buf, false),
    }
}

pub async fn pump<S>(
    ws: WebSocketStream<S>,
    mut input: mpsc::Receiver<Vec<u8>>,
    out: &mut impl Write,
) -> Result<Exit>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut tx, mut rx) = ws.split();
    loop {
        // Show what the VM sent before the next key is handled.
        tokio::select! {
            biased;
            msg = rx.next() => match msg {
                Some(Ok(Message::Binary(data))) => {
                    out.write_all(&data)?;
                    out.flush()?;
                }
                Some(Ok(Message::Text(text))) => {
                    out.write_all(text.as_bytes())?;
                    out.flush()?;
                }
                Some(Ok(Message::Close(frame))) => {
                    let reason = frame.map(|f| f.reason.to_string()).filter(|r| !r.is_empty());
                    return Ok(Exit::Closed(reason));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(anyhow!("console connection lost: {e}")),
                None => return Ok(Exit::Closed(None)),
            },
            data = input.recv() => match data {
                Some(buf) => {
                    let (send, escaped) = split_escape(&buf);
                    if !send.is_empty() {
                        tx.send(Message::Binary(send.to_vec().into())).await?;
                    }
                    if escaped {
                        let _ = tx.close().await;
                        // Let the server answer the close, so that the last input is not lost in a reset.
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            async { while rx.next().await.is_some() {} },
                        )
                        .await;
                        return Ok(Exit::Escaped);
                    }
                }
                None => return Ok(Exit::Escaped),
            },
        }
    }
}

struct RawMode;

impl RawMode {
    fn enable() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub async fn run(url: &Url) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("The console needs a terminal.");
    }
    let (ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .map_err(|e| anyhow!("cannot open the console: {e}"))?;
    eprintln!("Connected. Press Ctrl-] to leave. Press Enter to get a prompt.");
    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    // A blocking read cannot be cancelled. The thread ends with the process.
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 1024];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || tx.blocking_send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let exit = {
        let _raw = RawMode::enable()?;
        let mut stdout = std::io::stdout();
        pump(ws, rx, &mut stdout).await
    };
    eprintln!();
    match exit? {
        Exit::Escaped => eprintln!("Console closed."),
        Exit::Closed(Some(reason)) => bail!("The console was closed: {reason}"),
        Exit::Closed(None) => eprintln!("The console was closed by the server."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn escape_key_cuts_the_input() {
        assert_eq!(split_escape(b"ls\n"), (&b"ls\n"[..], false));
        assert_eq!(split_escape(b"ab\x1dcd"), (&b"ab"[..], true));
        assert_eq!(split_escape(b"\x1d"), (&b""[..], true));
    }

    async fn pair() -> (
        WebSocketStream<tokio::net::TcpStream>,
        WebSocketStream<tokio::net::TcpStream>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(s).await.unwrap()
        });
        let s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (client, _) = tokio_tungstenite::client_async(format!("ws://{addr}/c"), s)
            .await
            .unwrap();
        (client, server.await.unwrap())
    }

    #[tokio::test]
    async fn output_goes_to_the_terminal_and_input_to_the_vm_until_escape() {
        let (client, mut server) = pair().await;
        let (tx, rx) = mpsc::channel(8);
        server
            .send(Message::Binary(b"login: ".to_vec().into()))
            .await
            .unwrap();
        let pumping = tokio::spawn(async move {
            let mut out = Vec::new();
            let exit = pump(client, rx, &mut out).await;
            (exit, out)
        });
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tx.send(b"root\n".to_vec()).await.unwrap();
            server.next().await.unwrap().unwrap()
        })
        .await
        .unwrap();
        assert_eq!(first.into_data().as_ref(), b"root\n");
        tx.send(b"x\x1dignored".to_vec()).await.unwrap();
        let second = server.next().await.unwrap().unwrap();
        assert_eq!(second.into_data().as_ref(), b"x");
        // Read the close frame; the library answers it.
        let _ = server.next().await;
        let (exit, out) = pumping.await.unwrap();
        assert_eq!(exit.unwrap(), Exit::Escaped);
        assert_eq!(out, b"login: ");
    }

    #[tokio::test]
    async fn server_close_reason_is_returned() {
        let (client, mut server) = pair().await;
        let (_tx, rx) = mpsc::channel(1);
        server
            .close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: 1011.into(),
                reason: "Console is not available.".into(),
            }))
            .await
            .unwrap();
        let mut out = Vec::new();
        let exit = pump(client, rx, &mut out).await.unwrap();
        assert_eq!(exit, Exit::Closed(Some("Console is not available.".into())));
    }
}
