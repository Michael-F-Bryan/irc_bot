#[macro_use]
extern crate slog;

use actix::actors::signal::{ProcessSignals, Subscribe};
use actix::{Actor, System};
use failure::Error;
use irc::client::prelude::{Config as IrcConfig, IrcClient};
use irc_bot::messages::StartListening;
use irc_bot::{Bot, PanicHook, World};
use slog::{Drain, Level};
use std::process;
use structopt::StructOpt;

fn run(args: Args, logger: &slog::Logger) -> Result<(), Error> {
    info!(logger, "Application started");

    let irc_config = IrcConfig {
        nickname: Some(args.nick),
        server: Some(args.server),
        channels: Some(args.channels),
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

    let _bot = Bot::spawn(logger.clone(), &world, args.identify);

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
    let args = Args::from_args();
    let logger = initialize_logging(args.verbosity);

    if let Err(e) = run(args, &logger) {
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

fn initialize_logging(verbosity: usize) -> slog::Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let level = match verbosity {
        0 => Level::Info,
        1 => Level::Debug,
        _ => Level::Trace,
    };
    let drain = drain.filter_level(level).fuse();

    slog::Logger::root(drain, o!())
}

#[derive(StructOpt)]
pub struct Args {
    #[structopt(
        short = "n",
        long = "nick",
        help = "The nickname to use",
        default_value = "Michael-F-Bryan"
    )]
    pub nick: String,
    #[structopt(
        short = "i",
        long = "identify",
        help = "The password to use when identifying with the Mozilla IRC server"
    )]
    pub identify: String,
    #[structopt(
        short = "s",
        long = "server",
        help = "The server to connect to",
        default_value = "irc.mozilla.org"
    )]
    pub server: String,
    #[structopt(
        short = "c",
        long = "channel",
        help = "The channels to join on startup"
    )]
    pub channels: Vec<String>,
    #[structopt(
        short = "v",
        long = "verbose",
        help = "Enable more verbose output",
        parse(from_occurrences)
    )]
    pub verbosity: usize,
}
