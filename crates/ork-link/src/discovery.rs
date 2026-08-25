//! Finding a machine on the same network, so nobody has to type an address.
//!
//! One machine shouts on the local network; any machine lending a model
//! answers with its name and address. That is the whole of it.
//!
//! This works on a shared network and nowhere else. Broadcast traffic does not
//! leave the network it was sent on, which is a limitation and also the point:
//! a machine somewhere else on the internet is not going to answer, and should
//! not. To reach one of those you still need a private network -- Tailscale,
//! WireGuard, or a tunnel -- and then you type its address once. See the
//! documentation on linking machines.
//!
//! The reply carries no secret. It says "a machine here lends models, this is
//! what it is called, this is where it is" -- exactly what someone on the same
//! network could find with a port scan anyway. Getting a link still requires
//! the pairing code, which is not broadcast and never leaves the screen it is
//! shown on.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

/// What a searching machine sends. Versioned so a future format can change
/// without confusing an older build.
const PROBE: &[u8] = b"OUTLAW-DISCOVER-v1";

/// How long to listen for answers. Long enough for a home network, short
/// enough that nobody wonders whether it has frozen.
const LISTEN_FOR: Duration = Duration::from_millis(1500);

/// A machine that answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovered {
    pub machine_id: String,
    pub name: String,
    pub platform: String,
    pub version: String,
    /// Whether it is showing a pairing code right now.
    pub pairing_open: bool,
    /// Filled in from where the answer came from, not from what it claimed.
    #[serde(default)]
    pub address: String,
}

/// Answer discovery probes until `shutdown` completes.
pub async fn respond(
    port: u16,
    describe: impl Fn() -> Discovered + Send + 'static,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<()> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
        .await
        .with_context(|| format!("could not listen for discovery on port {port}"))?;
    socket.set_broadcast(true).ok();

    let serve = async {
        let mut buffer = [0u8; 64];
        loop {
            let Ok((length, from)) = socket.recv_from(&mut buffer).await else {
                continue;
            };
            if &buffer[..length] != PROBE {
                continue;
            }
            let reply = serde_json::to_vec(&describe()).unwrap_or_default();
            if let Err(error) = socket.send_to(&reply, from).await {
                tracing::debug!(%error, "could not answer a discovery probe");
            }
        }
    };

    tokio::select! {
        _ = serve => Ok(()),
        _ = shutdown => Ok(()),
    }
}

/// Ask the local network who is lending a model.
///
/// Never fails for want of an answer: an empty list means nobody replied,
/// which is an ordinary thing to find out.
pub async fn search(port: u16) -> Result<Vec<Discovered>> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .context("could not open a socket to search the network")?;
    socket
        .set_broadcast(true)
        .context("this system does not allow network broadcasts")?;

    let target = SocketAddrV4::new(Ipv4Addr::BROADCAST, port);
    socket
        .send_to(PROBE, target)
        .await
        .context("could not send the search")?;

    let mut found: Vec<Discovered> = Vec::new();
    let deadline = tokio::time::Instant::now() + LISTEN_FOR;
    let mut buffer = [0u8; 2048];

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, socket.recv_from(&mut buffer)).await {
            Ok(Ok((length, from))) => {
                let Ok(mut reply) = serde_json::from_slice::<Discovered>(&buffer[..length]) else {
                    continue;
                };
                // Where it answered from, not where it says it is. A machine
                // does not get to nominate its own address.
                reply.address = format!("http://{}:{port}", from.ip());
                if !found.iter().any(|seen| seen.machine_id == reply.machine_id) {
                    found.push(reply);
                }
            }
            Ok(Err(error)) => tracing::debug!(%error, "a discovery reply could not be read"),
            // Nobody else is going to answer.
            Err(_) => break,
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU16, Ordering};

    /// Each test needs its own port, or they answer each other's probes.
    fn test_port() -> u16 {
        static NEXT: AtomicU16 = AtomicU16::new(47_341);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    fn describe(name: &str) -> Discovered {
        Discovered {
            machine_id: format!("id-{name}"),
            name: name.to_string(),
            platform: "test".into(),
            version: "0.4.0".into(),
            pairing_open: true,
            address: String::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_machine_that_is_listening_is_found() {
        let port = test_port();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let responder = tokio::spawn(async move {
            let _ = respond(port, || describe("main pc"), async {
                let _ = stopped.await;
            })
            .await;
        });

        // Give the responder a moment to bind before shouting at it.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let found = search(port).await.unwrap();

        let _ = stop.send(());
        responder.abort();

        assert_eq!(found.len(), 1, "expected one machine, got {found:?}");
        assert_eq!(found[0].name, "main pc");
        assert!(found[0].pairing_open);
        // The address comes from the packet's sender, never from its contents.
        assert!(
            found[0].address.ends_with(&format!(":{port}")),
            "{}",
            found[0].address
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn finding_nobody_is_not_an_error() {
        let found = search(test_port()).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stray_datagram_is_ignored_rather_than_answered() {
        let port = test_port();
        let responder = tokio::spawn(async move {
            let _ = respond(port, || describe("main pc"), std::future::pending()).await;
        });
        tokio::time::sleep(Duration::from_millis(150)).await;

        let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        socket
            .send_to(b"hello?", SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await
            .unwrap();

        let mut buffer = [0u8; 512];
        let answered =
            tokio::time::timeout(Duration::from_millis(400), socket.recv_from(&mut buffer)).await;
        responder.abort();
        assert!(
            answered.is_err(),
            "it replied to something that was not a probe"
        );
    }
}
