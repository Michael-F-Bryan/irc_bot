use actix::msgs::StopArbiter;
use actix::{Actor, Addr, Arbiter, Context, Handler};
use crate::messages::{Connected, Identify, PrivateMessage, Registration};
use crate::World;
use failure::Error;
use futures::future::{self, Future};
use irc::client::Client;
use slog::Logger;

#[derive(Clone)]
pub struct Bot<C: Client + 'static> {
    logger: Logger,
    world: Addr<World<C>>,
    identify_password: String,
}

impl<C: Client + 'static> Bot<C> {
    fn new(
        logger: Logger,
        world: Addr<World<C>>,
        identify_password: String,
    ) -> Bot<C> {
        Bot {
            logger,
            world,
            identify_password,
        }
    }

    /// Soawn a [`Bot`] actor in the background.
    pub fn spawn(
        logger: Logger,
        world: &Addr<World<C>>,
        identify_password: String,
    ) -> Addr<Bot<C>> {
        let bot = Bot::new(logger, world.clone(), identify_password);
        let bot = bot.start();

        world.do_send(Registration::<Connected>::register(
            bot.clone().recipient(),
        ));

        bot
    }
}

impl<C: Client + 'static> Actor for Bot<C> {
    type Context = Context<Bot<C>>;
}

impl<C: Client + 'static> Handler<Connected> for Bot<C> {
    type Result = ();

    fn handle(&mut self, _msg: Connected, _ctx: &mut Self::Context) {
        info!(self.logger, "Connected to server");

        let world = self.world.clone();
        let logger = self.logger.clone();
        let identify_password = self.identify_password.clone();

        let fut = lift_err(self.world.send(Identify));
        let fut = lift_err(fut.and_then(move |_| {
            world
                .send(PrivateMessage {
                    to: String::from("NickServ"),
                    content: format!("IDENTIFY {}", identify_password),
                })
                .map_err(Error::from)
        }));

        Arbiter::spawn(fut.map_err(move |e: Error| {
            error!(logger, "Unable to identify"; "error" => e.to_string());
            Arbiter::current().do_send(StopArbiter(1));
        }));
    }
}

/// Convert a future which returns a result into a future which will error when
/// the inner result errors.
fn lift_err<T, E>(
    fut: impl Future<Item = Result<T, impl Into<E>>, Error = impl Into<E>>,
) -> impl Future<Item = T, Error = E> {
    fut.map_err(Into::into)
        .then(|item| item.map(|inner| inner.map_err(Into::into)))
        .flatten()
}
