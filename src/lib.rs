#[macro_use]
extern crate slog;
#[macro_use]
extern crate actix;

mod bot;
mod channel;
pub mod client;
pub mod logging;
mod utils;

pub use crate::bot::Bot;
pub use crate::client::spawn_client;
pub use crate::utils::PanicHook;
