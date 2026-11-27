//! Prints a new VAPID private key for `VAPID_PRIVATE_KEY` (T4.2.4).
//!
//! `cargo run -p notifications --example generate_vapid_key`
//!
//! Keep it in the server's `.env` only. Changing it later invalidates every browser
//! subscription: users turn push on again.

#![allow(clippy::print_stdout)]

fn main() {
    println!(
        "VAPID_PRIVATE_KEY={}",
        notifications::push::VapidKey::generate_private_key()
    );
}
