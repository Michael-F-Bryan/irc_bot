#[macro_use]
extern crate slog;

use actix::actors::signal::{ProcessSignals, Subscribe};
use actix::{Actor, System};
use failure::Error;
use irc::client::prelude::{Config as IrcConfig, IrcClient};
use irc_bot::messages::StartListening;
use irc_bot::{Bot, PanicHook, World};
use slog::Drain;
use std::process;

fn run(logger: &slog::Logger) -> Result<(), Error> {
    info!(logger, "Starting");

    let irc_config = IrcConfig {
        nickname: Some("Michael-F-Bryan".to_owned()),
        server: Some("irc.mozilla.org".to_owned()),
        channels: Some(vec![String::from("#rust-beginners")]),
        ..Default::default()
    };

    let client = IrcClient::from_config(irc_config).unwrap();
    let logger = logger.clone();

    let sys = System::new("irc-bot");
    let world = World::new_with_logger(client, logger.clone()).start();

    // set up signal and panic handling
    System::current()
        .registry()
        .get::<ProcessSignals>()
        .do_send(Subscribe(world.clone().recipient()));
    let _panic = PanicHook::new(world.clone());

    let _bot = Bot::spawn(logger.clone(), &world);

    world.do_send(StartListening);
    debug!(logger, "Telling the world to start listening for messages");

    if sys.run() == 0 {
        Ok(())
    } else {
        Err(failure::err_msg(
            "The system exited with a non-zero error code",
        ))
    }
}

fn main() {
    let logger = initialize_logging();

    if let Err(e) = run(&logger) {
        error!(logger, "Execution failed"; "error" => e.to_string());

        for cause in e.iter_causes() {
            warn!(logger, "Caused by: {}", cause.to_string());
        }

        drop(logger);
        let bt = e.backtrace().to_string();
        if !bt.is_empty() {
            eprintln!("{}", bt);
        }

        process::exit(1);
    }
}

fn initialize_logging() -> slog::Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    slog::Logger::root(drain, o!())
}
