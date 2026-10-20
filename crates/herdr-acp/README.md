# herdr-acp

Runs a coding agent through the [Agent Client Protocol](https://agentclientprotocol.com)
in a terminal pane, draws the conversation there, and lets Herdr UI attach to it
and show it as a native chat.

```sh
herdr-acp claude             # or codex, copilot, gemini, opencode
herdr-acp -- goose acp       # any ACP agent command
herdr-acp --load ID codex    # continue one of the agent's sessions
```

`herdr-acp --help` lists every option and key.

## Why a program in the pane

Herdr owns terminals, not conversations. The agent's structured conversation
has to live in something Herdr already keeps running, so it lives in this
program: Herdr starts it in a pane like any other command, keeps it alive when
Herdr UI closes, and the Herdr TUI shows its chat because it is an ordinary
terminal program. Nothing reads an agent's logs or screen, and nothing is
specific to one agent beyond the command that starts its ACP server.

`herdr-acp` is the only ACP client of the agent. Every view is a client of
`herdr-acp` instead:

- **The pane itself.** The chat drawn in the terminal, so the session works in
  the Herdr TUI, a plain terminal, or over SSH.
- **Herdr UI.** On a local daemon, it finds the pane's `acp` metadata token,
  connects to the endpoint it names, and draws the same conversation natively
  over the pane. Both views stay live: a prompt or a permission answer from
  either one shows in both, and the first answer to a permission request wins.
- **Anything else that speaks the attach protocol** in
  `crates/herdr-chat-protocol`.

## What it tells Herdr

Inside a Herdr pane (`HERDR_PANE_ID` and `HERDR_SOCKET_PATH` set), it reports
through Herdr's JSON API:

- `pane.report_agent` with the source `herdr-acp`, the agent label `acp`, and
  the state `idle`, `working`, or `blocked`, so the sidebar and notifications
  are exact rather than detected from the screen;
- `pane.report_metadata` with the agent's display name and the `acp` token, the
  attach socket's id;
- on exit, the token is cleared and the agent released.

It also writes OSC 7501 program status to the terminal, for terminals that read
it; terminals that do not ignore it.

## Agents

| Name | Started as |
| --- | --- |
| `claude` | `claude-agent-acp`, else `npx -y @agentclientprotocol/claude-agent-acp` |
| `codex` | `codex-acp`, else `npx -y @agentclientprotocol/codex-acp` |
| `copilot` | `copilot --acp` |
| `gemini` | `gemini --acp` |
| `opencode` | `opencode acp` |

Arguments after `--` go to the agent. Each agent signs in its own way; when it
reports that it needs authentication, the chat says how.

## The chat

The header shows the agent, whether it works and for how long, its model,
thinking level, and mode, how full its context window is, and what the
session has cost when the agent says. Each finished turn leaves a line saying
how long it took and the tokens it used. Settings the agent offers through
ACP's session config options (model, thinking level, mode, and any others)
change from **Ctrl+O** here and from the header's buttons in Herdr UI; the
agent's slash commands complete after `/`, and Up recalls earlier prompts.

## Behaviour

- Prompts sent while the agent works are queued and sent in order; cancelling
  ends the turn, declines pending permission requests, and drops the queue.
- The agent runs in its own process group and stops with the program.
- The program advertises no file system, terminal, or elicitation support, so
  agents use their own tools; it answers only permission requests.
- The transcript keeps at most 2,000 entries and 2 MiB of text, dropping the
  oldest, so a snapshot always fits one frame.

## Limits

- Herdr UI attaches through `herdr_client::private_endpoint`: on Unix a
  socket in `$XDG_STATE_HOME/herdr/acp` (or `~/.local/state/herdr/acp`), on
  Windows the named pipe `\\.\pipe\herdr-acp-<id>`, either one reachable by
  this user alone, so only from a Herdr UI on the same machine.
- A chat that is killed rather than quit leaves its Unix socket file behind;
  the next chat to start removes sockets nothing answers on. A pipe goes away
  with its process.
- Resuming after a restart needs `--load` with the agent's session id, and an
  agent that supports `session/load`.
