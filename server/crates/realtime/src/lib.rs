//! `realtime`: WebSocket hub, channel subscriptions and the LISTEN/NOTIFY bridge
//! (T0.5.1, ARCHITECTURE §10.1).
//!
//! # Protocol (`GET /ws`, JSON text frames tagged by `type`)
//! 1. The handshake is authenticated like any request: `Authorization: Bearer` (native apps)
//!    or the session cookie (web; then `Origin` must be an allowed web origin).
//! 2. The server sends `hello` with its clock (`server_time`) and the heartbeat interval.
//! 3. The client `subscribe`s to channels (`user:<own id>` for now) and sends `ping` every
//!    `heartbeat_secs`; the server answers `pong` and closes a silent connection after
//!    `idle_timeout_secs` (close code 4000).
//! 4. Events arrive as `event {channel, event: {name, data}}`. They only say *what changed*;
//!    data is refetched over HTTP.
//!
//! Other modules publish through [`Hub::publish`]; across instances the LISTEN/NOTIFY bridge
//! (T0.5.2) feeds the same hub.

pub mod hub;
pub mod protocol;
pub mod ws;

pub use hub::Hub;
pub use protocol::{Channel, ClientMessage, Event, ServerMessage};
pub use ws::{RealtimeConfig, RealtimeState, router};
