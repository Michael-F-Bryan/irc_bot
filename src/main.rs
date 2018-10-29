#[macro_use]
extern crate slog;

use actix::{Actor, Addr, Arbiter, Context, Handler, System};
use failure::Error;
use futures::Future;
use irc::client::prelude::Config as IrcConfig;
use irc_bot::client::{
    Client, Connected, Identify, Join, PrivateMessage, Registration,
};
use irc_bot::logging::{Logger, Oops};
use irc_bot::{self, PanicHook};
use slog::Drain;
use std::process;

fn run(logger: &slog::Logger) -> Result<(), Error> {
    let irc_config = IrcConfig {
        nickname: Some("test-bot".to_owned()),
        server: Some("irc.mozilla.org".to_owned()),
        channels: Some(vec![String::from("#rust-bots")]),
        ..Default::default()
    };

    let logger = logger.clone();
    let sys = System::new("irc-bot");

    let error_logger = Logger::from(logger.clone()).start();
    let _panic = PanicHook::new(error_logger.clone());

    if let Err(e) = irc_bot::spawn_client(
        logger,
        error_logger.clone(),
        register_bot,
        irc_config,
    ) {
        error_logger.do_send(Oops::fatal(e));
    }

    sys.run();

    Ok(())
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

/// Do some stuff immediately after creating the client so we can hook into the
/// IRC actor system.
fn register_bot(
    client: &Addr<Client>,
    error_logger: &Addr<Logger>,
    registration: &mut Registration,
) {
    let bot = Bot::new(client.clone(), error_logger.clone()).start();
    let recipient = bot.recipient();

    registration.register::<Connected>(recipient.clone());
}

#[derive(Clone)]
struct Bot {
    client: Addr<Client>,
    error_logger: Addr<Logger>,
}

impl Bot {
    pub fn new(client: Addr<Client>, error_logger: Addr<Logger>) -> Bot {
        Bot {
            client,
            error_logger,
        }
    }
}

impl Actor for Bot {
    type Context = Context<Bot>;
}

impl Handler<Connected> for Bot {
    type Result = ();

    fn handle(&mut self, _msg: Connected, _ctx: &mut Self::Context) {
        println!("Connected!");

        let err = self.error_logger.clone();

        let fut = self.client.send(Identify);

        let client = self.client.clone();
        let fut = fut.and_then(move |_| {
            client.send(PrivateMessage {
                to: String::from("nickserv"),
                content: String::from(":identify p9UG5eTbQ5kJp4Gq"),
            })
        });

        let client = self.client.clone();
        let fut = fut
            .and_then(move |_| {
                client.send(Join {
                    channels: String::from("#rust-bots"),
                })
            })
            .map_err(move |e| err.do_send(Oops::new(e)));

        Arbiter::spawn(fut);
    }
}
