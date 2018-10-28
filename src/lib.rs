#[macro_use]
extern crate slog;
#[macro_use]
extern crate actix;

pub mod logging;
mod utils;

pub use crate::utils::PanicHook;

use actix::{Actor, Addr, Arbiter, Context, Handler};
use crate::logging::{Logger, Oops};
use failure::Error;
use futures::future;
use irc::client::prelude::{
    Client as _Client, Config, Future, IrcClient, Stream,
};
use irc::client::ClientStream;

/// A connection to the IRC server.
pub struct Client {
    inner: IrcClient,
    logger: slog::Logger,
}

/// Creates a new IRC client actor, making sure it will get sent any incoming
/// IRC messages.
pub fn spawn_client(
    logger: slog::Logger,
    error_logger: Addr<Logger>,
    cfg: Config,
) -> Result<Addr<Client>, Error> {
    let client = Client::connect(logger, cfg)?;
    let stream = client.stream();
    let client = client.start();

    let client_2 = client.clone();

    let fut = stream
        .map_err(Error::from)
        .and_then(move |msg| client_2.send(Message(msg)).map_err(Error::from))
        .then(move |outcome| match outcome {
            Ok(_) => future::ok(()),
            Err(e) => {
                error_logger.do_send(Oops::new(e));
                future::ok(())
            }
        })
        .for_each(|_| future::ok(()));
    Arbiter::spawn(fut);

    Ok(client)
}

impl Client {
    fn connect(logger: slog::Logger, cfg: Config) -> Result<Client, Error> {
        debug!(logger, "Started connecting");
        let inner = IrcClient::from_config(cfg)?;
        Ok(Client { inner, logger })
    }

    fn stream(&self) -> ClientStream {
        self.inner.stream()
    }
}

impl Actor for Client {
    type Context = Context<Client>;
}

impl Handler<Message> for Client {
    type Result = ();

    fn handle(
        &mut self,
        msg: Message,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let msg = msg.0;

        debug!(self.logger, "Received a message";
            "message" => &format_args!("{}", msg));
        unimplemented!()
    }
}

#[derive(Message)]
struct Message(pub irc::proto::Message);
