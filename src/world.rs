use actix::dev::ToEnvelope;
use actix::{
    Actor, Addr, AsyncContext, Context, Handler, Message, Recipient,
    StreamHandler,
};
use crate::channel::Channel;
use crate::utils::MessageBox;
use futures::Stream;
use irc::client::Client;
use irc::error::IrcError;
use irc::proto::message::Message as IrcMessage;
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};

/// The entire state of the world.
pub struct World<C> {
    hooks: MessageBox,
    channels: HashMap<String, Addr<Channel>>,
    client: C,
}

impl<C> World<C> {
    pub fn new(client: C) -> World<C> {
        World {
            client,
            hooks: MessageBox::new(),
            channels: HashMap::new(),
        }
    }

    fn publish<M>(&mut self, msg: M)
    where
        M: BotMessage + Message + Clone + Send + 'static,
        M::Result: Send,
    {
        self.hooks.send(msg);
    }
}

impl<C: 'static> Actor for World<C> {
    type Context = Context<World<C>>;
}

/// A marker trait for messages you can subscribe to.
pub trait BotMessage {}

/// Tell the IRC client to start listening for messages.
#[derive(Debug, Copy, Clone, Message)]
pub struct StartListening;

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

    fn handle(&mut self, item: RawMessage, _ctx: &mut Self::Context) {
        self.publish(item);
    }
}

/// Subscribe to a particular message.
#[derive(Clone, Message)]
pub struct Register<M>
where
    M: Message + Send + 'static,
    M::Result: Send,
{
    recipient: Recipient<M>,
}

impl<M> Register<M>
where
    M: Message + Send + 'static,
    M::Result: Send,
{
    pub fn new(recipient: Recipient<M>) -> Register<M> {
        Register { recipient }
    }

    pub fn for_actor<A>(addr: Addr<A>) -> Register<M>
    where
        A: Actor + Handler<M>,
        A::Context: ToEnvelope<A, M>,
    {
        Register::new(addr.recipient())
    }
}

impl<M, C> Handler<Register<M>> for World<C>
where
    M: Message + Clone + Send + 'static,
    M::Result: Send,
    C: 'static,
{
    type Result = ();

    fn handle(&mut self, msg: Register<M>, _ctx: &mut Self::Context) {
        self.hooks.register(msg.recipient);
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct RawMessage(pub IrcMessage);

impl BotMessage for RawMessage {}

impl<C: Debug> Debug for World<C> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let World {
            ref client,
            ref channels,
            hooks: _,
        } = *self;

        let hooks = "/* elided */";

        f.debug_struct("World")
            .field("client", &client)
            .field("hooks", &hooks)
            .field("channels", &channels)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix::actors::mocker::Mocker;
    use actix::{Arbiter, System};
    use futures::future::{self, Future};
    use futures::Stream;
    use irc::proto::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};

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
        let msg: Register<DummyMessage> =
            Register::new(mock.clone().recipient());
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

        sys.block_on(world.send(Register::for_actor(sub.clone())))
            .unwrap();

        let msg = RawMessage(IrcMessage::from(Command::INFO(None)));
        world.do_send(msg.clone());
        assert_eq!(sys.run(), 0);

        let got = got.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], msg);
    }
}
