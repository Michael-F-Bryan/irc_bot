use actix::msgs::StopArbiter;
use actix::{Actor, Addr, Arbiter, Context, Handler};
use crate::messages::{Connected, Identify, PrivateMessage, Registration};
use crate::World;
use futures::Future;
use irc::client::Client;
use slog::Logger;

#[derive(Clone)]
pub struct Bot<C: Client + 'static> {
    logger: Logger,
    world: Addr<World<C>>,
}

impl<C: Client + 'static> Bot<C> {
    fn new(logger: Logger, world: Addr<World<C>>) -> Bot<C> {
        Bot { logger, world }
    }

    /// Soawn a [`Bot`] actor in the background.
    pub fn spawn(logger: Logger, world: &Addr<World<C>>) -> Addr<Bot<C>> {
        let bot = Bot::new(logger, world.clone());
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
        info!(self.logger, "Connected!");

        let world = self.world.clone();
        let logger = self.logger.clone();

        let fut = self
            .world
            .send(Identify)
            .and_then(move |_| {
                world.send(PrivateMessage {
                    to: String::from("NickServ"),
                    content: String::from("IDENTIFY p9UG5eTbQ5kJp4Gq"),
                })
            })
            .map_err(move |e| {
                error!(logger, "Unable to identify"; "error" => e.to_string());
                Arbiter::current().do_send(StopArbiter(1));
                ()
            });

        Arbiter::spawn(fut);
    }
}
