use actix::{Actor, Addr, Arbiter, Context, Handler};
use crate::client::{
    Client, Connected, Identify, Join, NotRegistered, PrivateMessage,
    Registration,
};
use crate::logging::{Logger, Oops};
use futures::Future;

#[derive(Clone)]
pub struct Bot {
    logger: slog::Logger,
    client: Addr<Client>,
    error_logger: Addr<Logger>,
}

impl Bot {
    pub fn new(
        logger: slog::Logger,
        client: Addr<Client>,
        error_logger: Addr<Logger>,
    ) -> Bot {
        Bot {
            logger,
            client,
            error_logger,
        }
    }

    /// Do some stuff immediately after creating the client so we can hook into
    /// the IRC actor system.
    pub fn register(
        logger: slog::Logger,
        client: &Addr<Client>,
        error_logger: &Addr<Logger>,
        registration: &mut Registration,
    ) {
        let bot = Bot::new(logger, client.clone(), error_logger.clone());
        let bot = bot.start();

        registration.register::<Connected>(bot.clone().recipient());
        registration.register::<NotRegistered>(bot.clone().recipient());
    }
}

impl Actor for Bot {
    type Context = Context<Bot>;
}

impl Handler<Connected> for Bot {
    type Result = ();

    fn handle(&mut self, _msg: Connected, _ctx: &mut Self::Context) {
        info!(self.logger, "Connected!");

        let err = self.error_logger.clone();
        let client = self.client.clone();

        let fut = self
            .client
            .send(Identify)
            .and_then(move |_| {
                client.send(PrivateMessage {
                    to: String::from("NickServ"),
                    content: String::from("IDENTIFY p9UG5eTbQ5kJp4Gq"),
                })
            })
            .map_err(move |e| err.do_send(Oops::new(e)));

        Arbiter::spawn(fut);
    }
}

impl Handler<NotRegistered> for Bot {
    type Result = ();

    fn handle(&mut self, _msg: NotRegistered, _ctx: &mut Self::Context) {
        info!(self.logger, "Sending registration information");

        let client = self.client.clone();
        let err = self.error_logger.clone();

        let fut = client
            .send(PrivateMessage {
                to: String::from("NickServ"),
                content: String::from("IDENTIFY p9UG5eTbQ5kJp4Gq"),
            })
            .map_err(move |e| err.do_send(Oops::new(e)));

        Arbiter::spawn(fut);
    }
}
