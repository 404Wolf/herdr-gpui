# Sandboxes

How herdr-gpui gives agents isolated machines: containers on this computer,
Fly Sprites, and the existing cloud providers. User-facing behavior is in the
crate README's "Containers (Docker or Podman)", "Fly Sprites", and "Smart
Dispatch" sections; this page records the design and is for working on it.

## Decision: a sandbox is a device

Herdr owns terminals, sessions, and agents, and this client never changes
Herdr. So a sandbox is not a process the GUI supervises: it is a Linux machine
running its own Herdr daemon, saved as a cloud device and reached the way an
SSH host is, by running a command whose standard streams carry Herdr's bridge
(`herdr bridge`). Everything Herdr already does for a remote host then holds
inside the sandbox:

- PTYs, resize, signals, exit status, and output replay are the sandbox's own
  daemon's. A window that closes or reconnects never stops an agent there.
- Reconnecting runs the same command again; nothing replays commands.
- Workspaces, worktrees, and agents are created by that daemon, through the
  `herdr` CLI that Teleport's scripts already drive.

The GUI's part is the machine's lifecycle (create, start, stop, delete),
getting Herdr onto it, and moving Git state in and out with Teleport.

## What the original RFC asked for, and where it lives

| RFC component | Here |
| --- | --- |
| Provider registry, `SandboxProvider` | `cloud/providers.rs`: one `providers!` row per provider generates the name, logo, connector, background command, deletion, on-demand provisioning, and `cloud::ProviderError`. No async trait: everything blocks on workers. |
| Session manager (PTY, reconnect) | The sandbox's own Herdr daemon, through `herdr_client::connect_command`. |
| Exec / attach | The provider's command: `docker exec -i`, `ssh`, or this app as a relay (`azure/sandbox/relay.rs`, `sprites/relay.rs`). |
| Git sync | `teleport/`: `fresh.rs` (new worktree from a base commit, cloned or bundled), `carry.rs`, and Teleport itself, untracked files included. |
| Workspace lifecycle | `cloud/jobs.rs` (create with progress), `cloud/catalog.rs` (saved devices), provider `setup.rs` (create, delete, roll back a failed create). |
| Dirty-state warning before delete | `cloud/unsynced.rs`, shown by the sidebar's Remove dialog. |
| Lane-to-sandbox | `dispatch/`: the **New container** / **New sprite** tile; `dispatch/job.rs` provisions, then dispatches. |
| Idle timers, quotas | `container/idle.rs`, `[container] max_containers`; Azure's `cleanup.rs`; sprites sleep on their own. |
| Credential broker | `teleport/credentials.rs` sends the `gh` token to a host that cannot reach GitHub. No scoping or trust tiers yet (see below). |
| Port proxy | `port_forward/`, SSH hosts only. |

## Adding a provider

1. A `CloudProvider` variant and key in `herdr-client`, behind a feature.
2. A module with `connect(account, id, session, stop)`, `command(account,
   id, stop)` (a `Command` the bridge, installer, and scripts append one
   shell string to, as `ssh` does), and an `Error`.
3. A row in `cloud/providers.rs`. `deletion` makes Remove delete the machine
   after the unpushed-work check; `provision` makes the machine available to
   dispatch.
4. A Settings card, a config table, and the per-provider `match` arms the
   compiler asks for (Settings tabs, Add Device). Keep the setup job's
   failure path deleting what it made.

A command must carry bytes unchanged and report the remote exit code, or 255
when the connection failed, as `ssh` does. Teleport's scripts need stdin's
end of file to reach the remote command: Git bundles travel on stdin.

## Providers evaluated

Checked 2026-10-17 against each provider's published API description; see
the sources. "Stream" is whether a long-lived, binary-safe stdin/stdout
stream to an arbitrary command exists.

| Provider | Stream | Status here |
| --- | --- | --- |
| Docker / Podman | Yes: `exec -i` without a TTY | Shipped. Live-tested with Docker (OrbStack). Podman follows the same CLI; not live-tested. |
| Fly Sprites | Yes: exec WebSocket, one tag byte per frame, explicit stdin EOF | Shipped as experimental. Tested against a scripted stream; no live account. |
| E2B | Only through envd's Connect RPC (base64 JSON, one POST per stdin chunk); its CLI decodes output as text, which breaks binary data | Not built: costlier than Sprites for the same result. |
| OpenSandbox | Its published execd API streams output but takes no stdin; a PTY WebSocket appears only in a design note, with no stdin EOF | Not built: needs a self-hosted server to verify, and the missing EOF breaks Teleport's bundle transfer. |
| Azure Container Apps sandboxes | Exec WebSocket on a terminal only | Already shipped, with a relay that works around the terminal. |

Sources: [Sprites exec WebSocket](https://docs.fly.io/sprites/api/websockets/execute-command.md),
[Sprites lifecycle](https://docs.fly.io/sprites/concepts/lifecycle.md),
[E2B process API](https://github.com/e2b-dev/infra/blob/main/packages/envd/spec/process/process.proto),
[E2B CLI exec](https://docs.e2b.dev/cli/exec-command.md),
[OpenSandbox](https://github.com/opensandbox-group/OpenSandbox),
[podman exec](https://docs.podman.io/en/v5.0.3/markdown/podman-exec.1.html).

## Threat model

- **Containers** share this computer's kernel. They keep cooperating agents
  apart (no host files, no engine socket, no host network, an unprivileged
  user, `no-new-privileges`, CPU, memory, and process limits) but are not a
  boundary against hostile code. The card and README say so.
- **VMs and microVMs** (Azure VMs, sprites) isolate the kernel. They do not
  stop a credential deliberately given to them from leaving.
- **Credentials**: a provider's API token stays on this computer; relays read
  it from the credential store and send it only to the configured API root.
  Teleport sends the `gh` token to a host that cannot reach GitHub, for every
  provider alike.
- **Deleting** a machine is the default answer only where the provider can
  delete it, and only after the unpushed-work search; a machine that cannot be
  searched is called unchecked, never clean.

## Not done yet

- **Agent credentials in new sandboxes.** A fresh container or sprite has no
  signed-in agent, so fan-out does not offer them: every lane would fail its
  agent check. Doing this safely means choosing what may be copied into a
  sandbox (API keys, not session files) and per-device trust, which the `gh`
  token handling would follow too.
- **OpenSandbox**, until it can be run and its stdin end of file verified.
- **Orphans.** A crash between a provider creating a machine and the device
  being saved leaves the machine unlisted. Containers carry the
  `dev.herdr-gpui.device` label and sprites a `herdr-` name, so a reconcile
  pass can find them; nothing offers that yet.
- **Ports** for containers and sprites: forwarding needs an SSH host.
