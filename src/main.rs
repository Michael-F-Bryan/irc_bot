#[macro_use]
extern crate slog;

use actix::{Actor, System};
use failure::Error;
use irc::client::prelude::Config as IrcConfig;
use irc_bot::logging::{Logger, Oops};
use irc_bot::Bot;
use irc_bot::PanicHook;
use slog::Drain;
use std::process;

fn run(logger: &slog::Logger) -> Result<(), Error> {
    let irc_config = IrcConfig {
        nickname: Some("Michael-F-Bryan".to_owned()),
        server: Some("irc.mozilla.org".to_owned()),
        channels: Some(vec![String::from("#rust-beginners")]),
        ..Default::default()
    };

    let logger = logger.clone();
    let sys = System::new("irc-bot");

    let error_logger = Logger::from(logger.clone()).start();
    let _panic = PanicHook::new(error_logger.clone());

    if let Err(e) = irc_bot::spawn_client(
        logger.clone(),
        error_logger.clone(),
        |a, b, c| Bot::register(logger.clone(), a, b, c),
        irc_config,
    ) {
        error_logger.do_send(Oops::fatal(e));
    }

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
