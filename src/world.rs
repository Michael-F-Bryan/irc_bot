use actix::actors::signal::Signal;
use actix::msgs::StopArbiter;
use actix::{
    Actor, Addr, Arbiter, AsyncContext, Context, Handler, Message,
    StreamHandler, System,
};
use crate::channel::Channel;
use crate::messages::{
    Connected, Identify, Join, NotRegistered, Panic, PrivateMessage,
    PrivateMessageReceived, Quit, RawMessage, Registration, StartListening,
};
use crate::utils::MessageBox;
use irc::client::prelude::{Client, ClientExt};
use irc::error::IrcError;
use irc::proto::message::Message as IrcMessage;
use irc::proto::{Command, Response};
use slog::{Discard, Logger};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};

/// The entire state of the world.
pub struct World<C> {
    hooks: MessageBox,
    channels: HashMap<String, Addr<Channel>>,
    client: C,
    logger: Logger,
    message_count: usize,
}

impl<C> World<C> {
    pub fn new(client: C) -> World<C> {
        World::new_with_logger(client, Logger::root(Discard, o!()))
    }

    pub fn new_with_logger(client: C, logger: Logger) -> World<C> {
        World {
            client,
            logger,
            hooks: MessageBox::new(),
            channels: HashMap::new(),
            message_count: 0,
        }
    }

    fn publish<M>(&mut self, msg: M)
    where
        M: Message + Clone + Send + 'static,
        M::Result: Send,
    {
        self.hooks.send(msg)
    }
}

impl<C: 'static> Actor for World<C> {
    type Context = Context<World<C>>;
}

impl<C: Debug> Debug for World<C> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let World {
            ref client,
            ref channels,
            ref logger,
            ref hooks,
            message_count,
        } = *self;

        f.debug_struct("World")
            .field("client", client)
            .field("hooks", &format_args!("({} listeners)", hooks.len()))
            .field("channels", channels)
            .field("logger", logger)
            .field("message_count", &message_count)
            .finish()
    }
}

impl<C: Client + 'static> Handler<StartListening> for World<C> {
    type Result = ();

    fn handle(&mut self, _msg: StartListening, ctx: &mut Self::Context) {
        ctx.add_stream(self.client.stream());
    }
}

impl<C: 'static> StreamHandler<IrcMessage, IrcError> for World<C> {
    fn handle(&mut self, item: IrcMessage, ctx: &mut Self::Context) {
        ctx.notify(RawMessage(item));
    }
}

impl<C: 'static> Handler<RawMessage> for World<C> {
    type Result = ();

    fn handle(&mut self, msg: RawMessage, _ctx: &mut Self::Context) {
        debug!(self.logger, "Received a message";
            "prefix" => msg.0.prefix.as_ref(),
            "source-nick" => msg.0.source_nickname(),
            "command" => format_args!("{:?}", msg.0.command));

        if self.message_count == 0 {
            debug!(self.logger, "Notifying listeners that we've connected");
            self.publish(Connected);
        }
        self.message_count += 1;

        match msg.0.command {
            Command::Response(
                Response::ERR_NOTREGISTERED,
                ref args,
                ref suffix,
            ) => {
                self.publish(NotRegistered {
                    args: args.clone(),
                    suffix: suffix.clone(),
                });
            }
            Command::PRIVMSG(ref target, ref message) => {
                self.publish(PrivateMessageReceived {
                    msg_target: target.clone(),
                    content: message.clone(),
                    raw: msg.0.clone(),
                })
            }
            _ => {}
        }

        self.publish(msg);
    }
}

impl<C: Client + 'static> Handler<Quit> for World<C> {
    type Result = ();

    fn handle(&mut self, msg: Quit, _ctx: &mut Self::Context) {
        info!(self.logger, "Received a request to exit");

        if let Err(e) = self.client.send_quit(msg.msg) {
            error!(self.logger, "Unable to quit"; "error" => e.to_string());
        }

        System::current().stop();
    }
}

impl<C: Client + 'static> Handler<PrivateMessage> for World<C> {
    type Result = Result<(), IrcError>;

    fn handle(
        &mut self,
        msg: PrivateMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(self.logger, "Sending a private message";
            "recipient" => &msg.to,
            "content" => &msg.content);

        let got = self.client.send_privmsg(msg.to, msg.content);

        if let Err(ref e) = got {
            error!(self.logger, "Unable to send a private message";
                "error" => e.to_string());
        }

        got
    }
}

impl<C: Client + 'static> Handler<Join> for World<C> {
    type Result = Result<(), IrcError>;

    fn handle(&mut self, msg: Join, _ctx: &mut Self::Context) -> Self::Result {
        self.client.send_join(&msg.channels)
    }
}

impl<C: Client + 'static> Handler<Identify> for World<C> {
    type Result = Result<(), IrcError>;

    fn handle(
        &mut self,
        _msg: Identify,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!(self.logger, "Sending identification");

        let got = self.client.identify();

        if let Err(ref e) = got {
            error!(self.logger, "Unable to identify";
                "error" => e.to_string());
        }

        got
    }
}

impl<C: 'static> Handler<Panic> for World<C> {
    type Result = ();

    fn handle(&mut self, msg: Panic, _ctx: &mut Self::Context) {
        let Panic {
            message,
            file,
            line,
            column,
            thread,
            backtrace,
        } = msg;

        let bt = backtrace.to_string();
        let bt = if bt.is_empty() { None } else { Some(bt) };

        error!(self.logger, "A thread panicked";
            "message" => message,
            "file" => file,
            "line" => line,
            "column" => column,
            "thread" => thread,
            "backtrace" => bt);
        Arbiter::current().do_send(StopArbiter(1));
    }
}

impl<C: Client + 'static> Handler<Signal> for World<C> {
    type Result = ();

    fn handle(&mut self, msg: Signal, _ctx: &mut Self::Context) {
        info!(self.logger, "Received a signal"; 
            "signal" => format_args!("{:?}", msg.0));

        if let Err(e) = self.client.send_quit("Leaving...") {
            error!(self.logger, "Encountered an error while trying to quit gracefully";
                "error" => e.to_string());
        }

        System::current().stop();
    }
}

macro_rules! allow_registration {
    ($message_type:ty) => {
        impl<C: 'static> Handler<Registration<$message_type>> for World<C> {
            type Result = ();

            fn handle(
                &mut self,
                msg: Registration<$message_type>,
                _ctx: &mut Self::Context,
            ) {
                msg.apply(&mut self.hooks);
            }
        }
    };
}

allow_registration!(RawMessage);
allow_registration!(Connected);

#[cfg(test)]
mod tests {
    use super::*;
    use actix::actors::mocker::Mocker;
    use actix::{Arbiter, System};
    use futures::future::{self, Future};
    use futures::Stream;
    use irc::proto::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, Message)]
    struct DummyMessage;

    impl<C: 'static> Handler<DummyMessage> for World<C> {
        type Result = ();

        fn handle(&mut self, msg: DummyMessage, _ctx: &mut Self::Context) {
            Arbiter::spawn(
                self.hooks
                    .do_send(msg)
                    .for_each(|_| future::ok(()))
                    .map_err(|e| panic!("{}", e)),
            );
        }
    }

    impl<C: 'static> Handler<Registration<DummyMessage>> for World<C> {
        type Result = ();

        fn handle(
            &mut self,
            msg: Registration<DummyMessage>,
            _ctx: &mut Self::Context,
        ) {
            msg.apply(&mut self.hooks);
        }
    }

    struct Sub<M> {
        received: Arc<Mutex<Vec<M>>>,
    }

    impl<M: 'static> Sub<M> {
        pub fn new() -> (Addr<Sub<M>>, Arc<Mutex<Vec<M>>>) {
            let received = Arc::new(Mutex::new(Vec::new()));
            let sub = Sub {
                received: Arc::clone(&received),
            };
            (sub.start(), received)
        }
    }

    impl<M: 'static> Actor for Sub<M> {
        type Context = Context<Sub<M>>;
    }

    impl<M> Handler<M> for Sub<M>
    where
        M: Message<Result = ()> + 'static,
    {
        type Result = ();

        fn handle(&mut self, msg: M, _ctx: &mut Self::Context) {
            self.received.lock().unwrap().push(msg);

            System::current().stop();
        }
    }

    #[test]
    fn register_and_receive_messages() {
        let mut sys = System::new("test");
        let world = World::new("this-is-a-client").start();
        let calls = Arc::new(AtomicUsize::default());
        let calls_2 = Arc::clone(&calls);

        let mock: Addr<Mocker<DummyMessage>> =
            Mocker::mock(Box::new(move |msg, _ctx| {
                assert!(msg.downcast_ref::<DummyMessage>().is_some());
                calls_2.fetch_add(1, Ordering::SeqCst);
                System::current().stop();
                Box::new(Some(<DummyMessage as Message>::Result::default()))
            }))
            .start();

        // tell the world we want to register for DummyMessages
        let msg: Registration<DummyMessage> =
            Registration::register(mock.clone().recipient());
        sys.block_on(world.send(msg)).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // then send a message and wait for it to arrive
        world.do_send(DummyMessage);
        assert_eq!(sys.run(), 0);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn raw_messages_are_forwarded_to_subscribers() {
        let mut sys = System::new("test");
        let world = World::new("asd").start();
        let (sub, got) = Sub::<RawMessage>::new();

        sys.block_on(world.send(Registration::for_actor(sub.clone(), true)))
            .unwrap();

        let msg = RawMessage(IrcMessage::from(Command::INFO(None)));
        world.do_send(msg.clone());
        assert_eq!(sys.run(), 0);

        let got = got.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], msg);
    }
}
