#[macro_use]
extern crate slog;
#[macro_use]
extern crate actix;

mod bot;
mod channel;
pub mod messages;
mod utils;
mod world;

pub use crate::bot::Bot;
pub use crate::utils::PanicHook;
pub use crate::world::World;
