# Remote control daemon

`herdr-remote` serves this machine's `herdr-acp` chats to paired phones over
HTTPS: a mobile web app (installable as a PWA) that lists workspaces, follows
chats, answers agent questions, and receives push notifications. The plan is
`docs/remote-control-plan.md`.

The daemon is a user service that Herdr UI installs when Remote control is
turned on, and it keeps running when the app quits. It never starts, stops,
or upgrades a Herdr daemon.

## Control protocol

`control` is the wire between Herdr UI and the daemon: one request and one
response per connection, as length-prefixed JSON frames
(`herdr_chat_protocol::write_frame`) over a private endpoint at
`control::address(state_dir)`. Herdr UI depends on this crate with
`default-features = false`, which leaves out the server and Tokio, and calls
`control::request` from a background thread.

## Pairing and authentication

`auth` (server only) holds the pairing the desktop dialog opens, the paired
devices in `devices.json` (0600, replaced atomically), challenge-signed
sessions, and per-address rate limits. It is synchronous and takes every time
as milliseconds since the Unix epoch, so the server calls it under a mutex and
tests pass explicit times.

## Errors

The crate exports `Error` and `Result<T>`.
