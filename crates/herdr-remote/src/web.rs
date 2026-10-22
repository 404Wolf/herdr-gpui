//! The web app phones use: a static shell (installable as a PWA) that
//! fetches server-rendered fragments and a stream of fragment patches with
//! a bearer token, and a JSON API for commands, pairing, and push.

mod render;
