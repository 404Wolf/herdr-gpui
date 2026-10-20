# Chat attach protocol

The wire between `herdr-acp`, the chat program that runs an ACP agent inside a
Herdr pane, and the clients that show its conversation: the program's own
terminal view and Herdr UI. The program is the only ACP client of the agent;
everything here is the conversation as it already projected it, so a client
needs no ACP types and no per-agent knowledge.

## Model

`Transcript` is the whole conversation a client draws: entries (user, agent,
thought, tool call, turn summary, notice), the turn status and when the running
turn started, pending permission requests, the plan, session modes and
settings (ACP config options: model, thinking level, mode, and others),
context window use and cost, queued prompts, and how many clients watch. `Change` is
one step from one transcript to the next, and `Transcript::apply` is the only
reducer: the program and every client run the same function, so they cannot
disagree about what a change means. A change that does not fit (an unknown
entry, a resolved permission that was never asked) is an error, and the client
answers it by asking for a fresh snapshot.

`assist` holds what every client does the same way with that model:
completing the agent's slash commands, recalling earlier prompts, and stepping
a setting. `display` formats durations, token counts, costs, and turn
summaries alike in every client.

## Wire

Frames are a little-endian `u32` byte length and a JSON payload, at most
`MAX_FRAME_SIZE` bytes. A client sends `Hello` with the protocol version and,
when reconnecting, the `Cursor` it last applied. The program answers `Welcome`,
then either the changes after that cursor or a `Snapshot`, then every change as
it happens. The cursor's epoch changes whenever the program starts or reloads
its session, so a cursor from an earlier run always gets a snapshot.

Clients send `Command`s with a client-chosen `CommandId`; the program answers
each once with `Ack`, and a command resent with an id it already handled is
acknowledged again without running twice. An `Ack` means the program accepted
the command, not that the agent finished acting on it.

A client that falls behind is disconnected rather than buffered without bound;
it reconnects with its cursor and gets the changes it missed, or a snapshot when
they are no longer kept.

## Discovery

The program reports a Herdr pane metadata token named `TOKEN` whose value is
its `EndpointId`. `endpoint_address(id)` turns the id into where it listens: a
socket in the private `endpoint_dir` on Unix, a named pipe on Windows. A client
never connects to a path read from daemon data, and connects through
`herdr_client::private_endpoint`, which keeps the endpoint to its user on
both platforms.

## Errors

The crate exports `Error` and `Result<T>`. Framing failures keep their I/O and
JSON sources; transcript failures name the entry or permission that did not
fit.
