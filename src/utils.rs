use actix::dev::ToEnvelope;
use actix::{Actor, Addr, Handler, Message, Recipient};
use anymap::AnyMap;
use failure::Backtrace;
use futures::stream::{self, Stream};
use std::any::Any;
use std::panic::{self, PanicInfo};
use std::thread;

/// A RAII guard which will forward any panics to some actor which can accept
/// the [`Panic`] message.
pub struct PanicHook {
    previous_handler: Option<Box<dyn Fn(&PanicInfo) + 'static + Sync + Send>>,
}

impl PanicHook {
    pub fn new<A: Handler<Panic>>(logger: Addr<A>) -> PanicHook
    where
        <A as Actor>::Context: ToEnvelope<A, Panic>,
    {
        let previous_handler = panic::take_hook();

        panic::set_hook(Box::new(move |panic_info| {
            logger.do_send(Panic::from(panic_info));
        }));

        PanicHook {
            previous_handler: Some(previous_handler),
        }
    }
}

impl Drop for PanicHook {
    fn drop(&mut self) {
        let previous_handler = self.previous_handler.take().unwrap();
        let _ = panic::take_hook();
        panic::set_hook(previous_handler);
    }
}

#[derive(Debug, Default, Message)]
pub struct Panic {
    pub message: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub thread: Option<String>,
    pub backtrace: Backtrace,
}

pub(crate) fn panic_message(payload: &(dyn Any + Send + 'static)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        msg.to_string()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        String::from("Panic")
    }
}

impl<'a> From<&'a PanicInfo<'a>> for Panic {
    fn from(other: &'a PanicInfo) -> Panic {
        let backtrace = Backtrace::new();
        let thread = thread::current().name().map(String::from);

        let message = panic_message(other.payload());

        match other.location() {
            Some(loc) => Panic {
                message,
                file: loc.file().to_string(),
                line: loc.line(),
                column: loc.column(),
                thread,
                backtrace,
            },
            None => Panic {
                message,
                backtrace,
                thread,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug)]
pub struct MessageBox {
    map: AnyMap,
}

impl MessageBox {
    pub fn new() -> MessageBox {
        MessageBox { map: AnyMap::new() }
    }

    pub fn register<M>(&mut self, recipient: Recipient<M>)
    where
        M: Message + Clone + Send + 'static,
        M::Result: Send,
    {
        let recipients = self
            .map
            .entry::<Vec<Recipient<M>>>()
            .or_insert_with(Default::default);

        recipients.push(recipient);
    }

    pub fn unregister<M>(&mut self, recipient: &Recipient<M>)
    where
        M: Message + Clone + Send + 'static,
        M::Result: Send,
    {
        if let Some(recipients) = self.map.get_mut::<Vec<Recipient<M>>>() {
            if let Some(ix) = recipients.iter().position(|x| *x == *recipient) {
                recipients.remove(ix);
            }
        }
    }

    pub fn send<M>(&self, msg: M)
    where
        M: Message + Clone + Send + 'static,
        M::Result: Send,
    {
        if let Some(recipients) = self.map.get::<Vec<Recipient<M>>>() {
            for recipient in recipients {
                let _ = recipient.do_send(msg.clone());
            }
        }
    }

    /// Send a copy of the message to each registered recipient, returning a
    /// stream of responses which will be resolved as they come in.
    pub fn do_send<M>(
        &self,
        msg: M,
    ) -> impl Stream<Item = M::Result, Error = actix::MailboxError>
    where
        M: Message + Clone + Send + 'static,
        M::Result: Send,
    {
        let recipients = match self.map.get::<Vec<Recipient<M>>>() {
            Some(r) => r.as_slice(),
            None => &[],
        };

        let futures = recipients
            .iter()
            .map(move |recipient| recipient.send(msg.clone()));

        stream::futures_unordered(futures)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for MessageBox {
    fn default() -> MessageBox {
        MessageBox::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix::{Context, System};

    #[derive(Debug, Clone, Copy, Message)]
    struct Ping;

    #[derive(Debug, Default, PartialEq)]
    struct PingReceiver {
        count: usize,
    }

    impl Actor for PingReceiver {
        type Context = Context<PingReceiver>;
    }

    impl Handler<Ping> for PingReceiver {
        type Result = ();

        fn handle(&mut self, _msg: Ping, _ctx: &mut Self::Context) {
            self.count += 1;
        }
    }

    #[derive(Debug, Copy, Clone)]
    struct PingCount;

    impl Message for PingCount {
        type Result = usize;
    }

    impl Handler<PingCount> for PingReceiver {
        type Result = usize;

        fn handle(
            &mut self,
            _msg: PingCount,
            _ctx: &mut Self::Context,
        ) -> Self::Result {
            self.count
        }
    }

    #[test]
    fn receive_a_message() {
        let mut sys = System::new("test");

        let pinger = PingReceiver::default();
        let mut map = MessageBox::new();
        let addr = pinger.start();

        assert!(map.is_empty());
        map.register::<Ping>(addr.clone().recipient());
        assert_eq!(1, map.len());

        map.send(Ping);

        let count = sys.block_on(addr.send(PingCount)).unwrap();
        assert_eq!(count, 1);
    }
}
