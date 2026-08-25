# Using another machine's model

The computer with a problem is often not the computer with a good graphics
card. An old laptop, a home server, a machine with 4 GB of video memory -- all
can run the checks perfectly well, because the checks are ordinary diagnostics
that need almost nothing. It is only the *explanation* step that wants a
capable model.

So: run the checks locally, and borrow a stronger machine for the thinking.

```
   weak machine                        strong machine
   ------------                        --------------
   runs every check   ---- network ---> runs the model
   builds findings    <--- answer -----
   fixes things
```

**This works over any network you can reach the other machine on.** A home LAN,
a VPN, a mesh network such as Tailscale, ZeroTier, or Nebula, an SSH tunnel, a
Docker network. The tool has no knowledge of how you connected the two
machines; it only needs a URL it can reach. Nothing in it is specific to any
particular networking product.

## Step 1: serve a model on the strong machine

Any OpenAI-compatible server. The one thing that differs from a purely local
setup is that it must listen on an address the other machine can reach, not
just `127.0.0.1`.

**Ollama**

```bash
# Linux, systemd
sudo systemctl edit ollama
```

```ini
[Service]
Environment="OLLAMA_HOST=0.0.0.0:11434"
```

```bash
sudo systemctl restart ollama
```

On Windows, set `OLLAMA_HOST` to `0.0.0.0:11434` in your environment variables
and restart Ollama.

**LM Studio**

In the Developer tab, start the server and enable "Serve on Local Network".
Note the port, usually 1234.

Check it from the strong machine itself first:

```bash
curl http://localhost:11434/v1/models
```

## Step 2: find the address the weak machine should use

Whatever address the two machines can reach each other on:

| Setup | Typical address |
| --- | --- |
| Home network | The machine's LAN IP, e.g. `192.168.1.20` |
| Tailscale | Its Tailscale IP (`100.x.y.z`), or its MagicDNS name |
| ZeroTier | Its ZeroTier-assigned IP |
| Any of the above | The hostname, if name resolution works between them |

Verify from the **weak** machine before configuring anything:

```bash
curl http://THE-ADDRESS:11434/v1/models
```

If that returns a list of models, you are done with the hard part. If it hangs
or refuses, see [Troubleshooting](troubleshooting.md#the-other-machine-cannot-be-reached).

## Step 3: point the weak machine at it

Edit `config.toml` (run `outlaw config` to find it):

```toml
[ai]
mode = "auto"

[ai.remote]
enabled = true

[ai.remote.endpoint]
url = "http://THE-ADDRESS:11434/v1"
model = ""      # empty means "whatever that server has loaded"
```

Check it:

```bash
outlaw models
```

You should see the remote tier selected, naming the model and address. If it
was not reachable, the output says so and why, and the tool falls through to
whatever else is available.

## Step 4 (optional): require a token

If your server is exposed somewhere less trusted, put a reverse proxy in front
of it that requires a bearer token, and give the tool the token:

```bash
outlaw set-key remote
```

It is stored in the operating system's credential store and sent as a bearer
token with each request.

## What this does and does not do

**Does:** send the findings from the weak machine to the strong machine's model
for explanation, and bring the answer back.

**Does not:** let the strong machine scan, control, or change the weak machine.
There is no remote control channel. Each machine runs its own checks and makes
its own changes locally. This is a deliberate limit -- it keeps the blast
radius of a misconfigured or compromised endpoint to "gives bad advice" rather
than "changes another computer".

## Security notes

- An OpenAI-compatible server usually has **no authentication at all**. Do not
  bind one to a public interface. On a LAN or a private mesh network this is
  usually fine; on a machine with a public IP it is not.
- The findings sent include machine details and log excerpts. On your own
  hardware that is the point; consider what is in them before sending them
  anywhere you do not control.
- `http://` is fine across a mesh network that already encrypts traffic
  (Tailscale, ZeroTier, WireGuard). Over a plain LAN, or anything wider, put
  TLS in front of it.

## Worked example

A gaming desktop with plenty of video memory, and an older laptop, joined by a
mesh network.

On the desktop:

```bash
OLLAMA_HOST=0.0.0.0:11434 ollama serve
ollama pull qwen3:14b
```

On the laptop, in `config.toml`:

```toml
[ai.remote]
enabled = true

[ai.remote.endpoint]
url = "http://desktop:11434/v1"
```

Then, on the laptop:

```bash
outlaw scan --explain
```

The laptop does all the checking. The desktop does the thinking.
