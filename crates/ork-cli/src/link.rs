//! `outlaw link` -- lending a model to another machine, and borrowing one.

use anyhow::{Context, Result};
use ork_core::Config;
use ork_link::client::{self, LinkClient};
use ork_link::peer::{PeerBook, Role};
use ork_link::server::{HostState, serve};
use ork_link::{PairingCode, discovery, routing};

use crate::style::{bold, dim};

fn book_path() -> Result<std::path::PathBuf> {
    PeerBook::default_path()
}

fn load_book() -> Result<PeerBook> {
    PeerBook::load(&book_path()?)
}

/// `outlaw link host` -- lend this machine's model to machines you pair with.
pub async fn host(port: u16, model_url: Option<String>, discoverable: bool) -> Result<()> {
    let path = book_path()?;
    let book = PeerBook::load(&path)?;
    let config = Config::load_or_default(&Config::default_path()?)?;

    let upstream = model_url
        .or_else(|| config.ai.local.urls.first().cloned())
        .context("no local model address is configured -- set one in `outlaw config` or pass --model-url")?;

    let name = book.machine_name.clone();
    let machine_id = book.machine_id.clone();
    let existing = book.peers.iter().filter(|peer| peer.role == Role::Borrower).count();
    let state = std::sync::Arc::new(HostState::new(book, path, upstream.clone()));

    println!("{}", bold("Lending a model"));
    println!("  {:<14}{name}", "this machine");
    println!("  {:<14}{upstream}", "model");
    println!("  {:<14}port {port}", "listening on");
    if existing > 0 {
        println!("  {:<14}{existing} machine(s) already linked", "known");
    }
    println!();

    let code = state.open_pairing();
    println!("{}", bold("Pairing code"));
    println!();
    println!("      {}", bold(&code.display()));
    println!();
    for line in [
        "Type that on the other machine, with:",
        "    outlaw link join",
        "",
        "It expires in ten minutes, works once, and stops accepting guesses",
        "after five wrong tries. Anyone who has it can ask this computer to",
        "run its model -- and nothing else. Nothing here can change this",
        "machine, because no such command exists in the link.",
    ] {
        println!("  {}", dim(line));
    }
    println!();
    println!("  {}", dim("Press Ctrl-C to stop lending."));
    println!();

    // Answering on the local network is what saves anyone typing an address.
    let discovery_task = discoverable.then(|| {
        let state = state.clone();
        let name = name.clone();
        let machine_id = machine_id.clone();
        tokio::spawn(async move {
            let describe = move || discovery::Discovered {
                machine_id: machine_id.clone(),
                name: name.clone(),
                platform: ork_core::platform::detect()
                    .map(|platform| platform.kind().to_string())
                    .unwrap_or_default(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                pairing_open: state.pairing_open(),
                address: String::new(),
            };
            if let Err(error) = discovery::respond(port, describe, std::future::pending()).await {
                tracing::warn!(%error, "not answering discovery on the network");
            }
        })
    });

    let address = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let result = serve(state, address, async {
        // Stopping is always the user's decision, here as everywhere else.
        let _ = tokio::signal::ctrl_c().await;
        println!();
        println!("  {}", dim("stopped lending"));
    })
    .await;

    if let Some(task) = discovery_task {
        task.abort();
    }
    result
}

/// `outlaw link find` -- who on this network is lending a model.
pub async fn find(port: u16, json: bool) -> Result<()> {
    let found = discovery::search(port).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }

    if found.is_empty() {
        println!("{}", bold("Nobody on this network is lending a model"));
        println!();
        for line in [
            "On the other machine, run:",
            "    outlaw link host",
            "",
            "A machine somewhere else will not answer this -- broadcasts do not",
            "leave the network they are sent on. For one of those, use its",
            "address: outlaw link join --at <address>",
        ] {
            println!("  {}", dim(line));
        }
        return Ok(());
    }

    println!("{}", bold("Lending a model on this network"));
    for machine in &found {
        let state = if machine.pairing_open { "showing a pairing code" } else { "not pairing right now" };
        println!("  {:<22}{}  {}", machine.name, machine.address, dim(state));
    }
    println!();
    println!("  {}", dim("Link one with: outlaw link join"));
    Ok(())
}

/// `outlaw link join` -- pair with a machine that is showing a code.
///
/// With no address it looks on the local network first, which is the case that
/// should need no typing at all.
pub async fn join(code: Option<String>, at: Option<String>, port: u16) -> Result<()> {
    let address = match at {
        Some(address) => client::normalise_address(&address),
        None => {
            println!("{}", dim("Looking for a machine on this network..."));
            let mut found = discovery::search(port).await?;
            found.retain(|machine| machine.pairing_open);

            match found.len() {
                0 => anyhow::bail!(
                    "no machine on this network is showing a pairing code.\n\
                     Run `outlaw link host` on the other machine, or give this one an\n\
                     address with --at if it is somewhere else."
                ),
                1 => {
                    println!("  found {} at {}", bold(&found[0].name), found[0].address);
                    found[0].address.clone()
                }
                // Picking one for them would be picking which computer they
                // trust, which is not a choice to make on somebody's behalf.
                _ => {
                    println!("{}", bold("More than one machine is showing a code"));
                    for machine in &found {
                        println!("  {:<22}{}", machine.name, machine.address);
                    }
                    anyhow::bail!("say which one with --at <address>");
                }
            }
        }
    };

    let code = match code {
        Some(text) => PairingCode::parse(&text)?,
        None => {
            eprint!("Pairing code: ");
            let mut typed = String::new();
            std::io::stdin().read_line(&mut typed).context("could not read the pairing code")?;
            PairingCode::parse(&typed)?
        }
    };

    let path = book_path()?;
    let mut book = PeerBook::load(&path)?;
    let peer = client::join(&mut book, &address, code).await?;
    book.save(&path)?;

    println!();
    println!("{}", bold("Linked"));
    println!("  {:<14}{} ({})", "machine", peer.name, peer.platform);
    println!("  {:<14}{}", "address", peer.address);
    println!("  {:<14}{}", "version", peer.version);
    println!();
    println!("  {}", dim("Its model will be used automatically. Check with: outlaw models"));
    Ok(())
}

/// `outlaw link` -- what this machine is linked to.
pub async fn show(json: bool, check: bool) -> Result<()> {
    let book = load_book()?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "machine_id": book.machine_id,
                "machine_name": book.machine_name,
                "peers": book.peers,
            }))?
        );
        return Ok(());
    }

    println!("{}", bold("This machine"));
    println!("  {:<14}{}", "name", book.machine_name);
    println!("  {:<14}{}", "id", dim(&book.machine_id));
    println!();

    if book.peers.is_empty() {
        println!("{}", bold("Not linked to anything"));
        println!();
        for line in [
            "To borrow a stronger machine's model, run this on that machine:",
            "    outlaw link host",
            "and then, here:",
            "    outlaw link join",
        ] {
            println!("  {}", dim(line));
        }
        return Ok(());
    }

    println!("{}", bold("Linked machines"));
    for peer in &book.peers {
        let where_ = if peer.address.is_empty() { dim("connects to us") } else { peer.address.clone() };
        println!("  {:<22}{:<16}{}", peer.name, peer.role.as_str(), where_);

        if check && peer.role == Role::Lender {
            // Asking is the only way to know. A stored link says nothing about
            // whether the machine is switched on.
            let answer = match LinkClient::for_peer(peer) {
                Ok(link) => match link.hello().await {
                    Ok(_) => "answering".to_string(),
                    Err(error) => format!("{error:#}"),
                },
                Err(error) => format!("{error:#}"),
            };
            println!("  {:<22}{}", "", dim(&answer));
        }
    }
    println!();

    // What the router will actually do with all this.
    let mut config = Config::load_or_default(&Config::default_path()?)?;
    match routing::apply_to_config(&book, &mut config) {
        Some(peer) => println!("  {}", dim(&format!("{} will be asked first when a model is needed", peer.name))),
        None if config.ai.remote.endpoint.is_some() => println!(
            "  {}",
            dim("a remote endpoint set by hand takes priority over any link")
        ),
        None => println!("  {}", dim("no link is currently usable for running a model")),
    }
    Ok(())
}

/// `outlaw link remove` -- cut a link, and forget its token with it.
pub fn remove(name: &str) -> Result<()> {
    let path = book_path()?;
    let mut book = PeerBook::load(&path)?;
    let removed = book.remove(name);
    anyhow::ensure!(!removed.is_empty(), "nothing here is linked as `{name}`");
    book.save(&path)?;

    for peer in &removed {
        println!("Unlinked {} ({}).", peer.name, peer.role.as_str());
    }
    println!(
        "{}",
        dim("Its access token has been removed from the credential store. The other machine keeps its own record until it removes the link too.")
    );
    Ok(())
}

/// `outlaw link view` -- what is wrong with a machine at the other end.
///
/// Read-only on purpose. This is the "that computer is across town" case: you
/// can see what it found, and then you go and deal with it.
pub async fn view(name: Option<String>, json: bool) -> Result<()> {
    let book = load_book()?;
    let peer = match &name {
        Some(name) => book.find(name).with_context(|| format!("nothing here is linked as `{name}`"))?,
        None => book
            .lenders()
            .next()
            .context("this machine is not linked to anything -- see `outlaw link join`")?,
    };

    let link = LinkClient::for_peer(peer)?;
    let status = link.status().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("{}", bold(status["host_name"].as_str().unwrap_or(&peer.name)));
    if let Some(host) = status.get("host") {
        println!("  {:<14}{}", "system", host["os_name"].as_str().unwrap_or("unknown"));
        println!("  {:<14}{}", "processor", host["cpu_brand"].as_str().unwrap_or("unknown"));
    }
    println!("  {:<14}{}", "version", status["version"].as_str().unwrap_or("unknown"));
    println!();

    // A machine whose queue could not be read is a fact worth stating, not a
    // silence to be mistaken for good news.
    if let Some(problem) = status["queue_error"].as_str() {
        println!("{}", bold("Could not read what it found"));
        println!("  {}", dim(problem));
        return Ok(());
    }

    let waiting = status["waiting"].as_array().cloned().unwrap_or_default();
    if waiting.is_empty() {
        println!("{}", bold("Nothing is waiting on that machine"));
        println!("  {}", dim("Run a scan over there to fill its queue."));
        return Ok(());
    }

    println!("{}", bold(&format!("{} problem(s) waiting", waiting.len())));
    for item in &waiting {
        println!(
            "  {:<10}{:<40}{}",
            item["severity"].as_str().unwrap_or(""),
            item["title"].as_str().unwrap_or(""),
            dim(item["subject"].as_str().unwrap_or("")),
        );
    }
    println!();
    println!(
        "  {}",
        dim("Fixing is done at that machine's own keyboard. Nothing in a link can change it.")
    );
    Ok(())
}
