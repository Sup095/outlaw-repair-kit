# Linking two machines

A computer with a weak graphics card can run every diagnostic check perfectly
well. What it cannot do is run a model worth asking. A stronger computer in the
same house can — and getting the two to talk should not require setting up a
private network first.

So: one machine shows a code, you type it on the other, and they know each
other from then on. On a shared network they find each other without anyone
typing an address at all.

## What a link can do, and what it cannot

A linked machine can be asked to **think about a problem** and to say **what
its last scan found**. That is the entire list.

It cannot be made to do anything. There is no command in the protocol that
changes the machine at the other end — not a blocked one, not a
permission you could grant later. It was never written. Fixing a computer is
something you do at that computer's keyboard.

If somebody steals a link token, what they have won is the ability to make your
computer answer questions. That is the whole prize.

## Lending a model

On the machine with the good graphics card, with LM Studio or Ollama already
running:

```bash
outlaw link host
```

It prints a pairing code and waits:

```text
Pairing code

      FG1M-PNXF-PSXW
```

The code lasts ten minutes, works **once**, and stops accepting guesses after
five wrong tries. Stop lending with Ctrl-C.

It tells you what happens as it happens — a machine linking, a wrong code and
how many tries are left, a model being run for somebody:

```text
  wrong pairing code -- 4 attempt(s) left
  linked work rig
  that machine can now ask this one to run its model
```

**One code links one machine.** To link a second, press Ctrl-C and run
`outlaw link host` again for a fresh code. In the desktop app there is a
**Show a new code** button, so the lending does not have to stop. The code is
not reopened automatically on purpose: a code nobody is watching is a standing
invitation.

## Borrowing one

On the other machine:

```bash
outlaw link join
```

With no address it looks on the local network, finds the machine showing a
code, and asks you for the code. That is the whole setup.

If the two are not on the same network, give it the address:

```bash
outlaw link join --at 100.64.0.2
```

Then check it:

```bash
outlaw link check
outlaw models
```

`outlaw models` should now show the remote tier chosen, with the linked
machine's name.

## What crosses the network

The access token is **never sent**. Both machines derive it from the pairing
code:

```text
  borrower -> lender   a random number, and a proof it knows the code
  lender   -> borrower a proof that it knows the code too
  both derive          the token, from the code and that random number
```

Somebody watching sees two proofs and a random number. Without the code they
cannot work out the token, and the code stops being accepted within minutes or
after five wrong guesses, whichever comes first.

The lender's proof matters as much as the borrower's: it is what stops another
machine on the same network from answering in the lender's place and being
handed a session.

Once linked, the token lives in each machine's own credential store —
Credential Manager on Windows, the desktop secret service on Linux. The list of
linked machines (`peers.json`) holds a *hash* of the token, never the token,
so a copied file is worth nothing.

## Finding machines

```bash
outlaw link find
```

This shouts on the local network and lists whoever answers. The reply carries
no secret — just a name and an address, which anyone on that network could find
with a port scan anyway. Getting an actual link still needs the code.

**Broadcasts do not leave the network they are sent on.** A machine somewhere
else on the internet will not answer this, and should not. To reach one of
those you still need a private network — Tailscale, WireGuard, ZeroTier, a
tunnel — and then you type its address once with `--at`. See
[remote-machine.md](remote-machine.md).

Linking does not replace that. It removes it from the common case.

## Seeing what is wrong over there

```bash
outlaw link view            # the first linked machine
outlaw link view main-pc    # a particular one
```

This is the "that computer is across town and something is wrong with it" case.
It shows what that machine is, and what its scan left waiting in its queue.

It is read-only, and that is the end of it: there is no route in the link that
changes the machine at the other end. Fixing happens at that machine's own
keyboard.

## Seeing and cutting links

```bash
outlaw link           # what this machine is linked to
outlaw link check     # ask each one whether it is still answering
outlaw link remove <name>
```

Removing a link deletes this machine's token for it. The other machine keeps
its own record until it removes the link too — cut it from both ends if you
mean it.

## How this fits with everything else

A linked machine fills in the **remote endpoint** the model router already had.
It is not a new tier and not a new code path:

1. A remote endpoint — a link, or one you typed in yourself
2. A local model, sized to this machine's graphics card
3. A cloud provider

An endpoint you set by hand always wins over a link. If you typed an address
into your settings, you meant it.

## From the window

The desktop app has a **Machines** screen that does all of this: show a pairing
code, find machines on the network, link, check, view, and unlink. Nobody
should have to open a terminal to pair two computers.

## Options

| | |
| --- | --- |
| `outlaw link host --port 7341` | Listen somewhere else |
| `outlaw link host --model-url http://127.0.0.1:11434/v1` | Lend a specific model |
| `outlaw link host --no-discovery` | Do not answer discovery on the network |
| `outlaw link join --at <address>` | Skip discovery and go straight there |
| `outlaw link find --port 7341` | Search a different port |

Port 7341 is the default, in both directions.

## When it does not work

**"no machine on this network is showing a pairing code"** — the other machine
is not running `outlaw link host`, its firewall is blocking UDP port 7341, or
the two are not on the same network. Use `--at <address>` to skip discovery
entirely; that only needs TCP.

**"that pairing code is not right"** — codes are single-use. If one has already
been used, press Ctrl-C on the lender and run `outlaw link host` again for a
fresh one.

**"no model is running on that machine"** — the link is fine; the lender's LM
Studio or Ollama is not running. Check with `outlaw models` over there.

**"this machine has no credential store running"** — there is nowhere safe to
keep the access token, so pairing stops before it starts rather than burning a
single-use code on a link that would not work. On a Linux desktop, start a
secret service (GNOME Keyring or KWallet). A headless server has none by
default.

**"that machine no longer recognises this one"** — the link was removed at the
other end. Pair again.
