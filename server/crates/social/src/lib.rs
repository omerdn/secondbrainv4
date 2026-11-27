//! `social`: friendships and blocks (PRD AK1, AK2, V1, T3.1.1); nudges and emoji reactions come
//! with E3.5.
//!
//! - [`Friendships`]: requests by e-mail, accept, decline, cancel, unfriend, block and unblock.
//!   A request to an unknown address or to someone who blocked the sender looks exactly like a
//!   pending one to the sender, so neither leaks (AK2). Requests are rate limited and a declined
//!   one waits before it can be sent again, with an injected `Clock`.
//! - The real `items::SharingPolicy` (V1): people may share when they are friends and neither
//!   blocked the other. Teams join in F5.

pub mod http;
mod model;
mod repo;
mod service;

pub use model::{
    BlockedUser, DECLINE_COOLDOWN, Friend, IncomingRequest, OutgoingRequest, REQUESTS_PER_DAY,
    SendFriendRequest, normalize_email,
};
pub use service::Friendships;
