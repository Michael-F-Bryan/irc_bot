#[macro_use]
extern crate slog;
#[macro_use]
extern crate actix;

pub mod client;
pub mod logging;
mod utils;

pub use crate::client::spawn_client;
pub use crate::utils::PanicHook;
