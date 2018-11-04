use actix::dev::ToEnvelope;
use actix::{Actor, Addr, Handler, Message, Recipient};
use crate::channel::Channel;
use crate::utils::MessageBox;
use failure::Backtrace;
use irc::error::IrcError;
use irc::proto::message::Message as IrcMessage;
use std::any::Any;
use std::collections::HashMap;
use std::panic::PanicInfo;
use std::thread;

#[derive(Debug, Clone, PartialEq, Message)]
pub struct RawMessage(pub IrcMessage);

/// Tell the IRC client to disconnect from the server and halt the actor system.
#[derive(Debug, Message)]
pub struct Quit {
    pub msg: String,
}

impl Quit {
    pub fn new<S: Into<String>>(msg: S) -> Quit {
        Quit { msg: msg.into() }
    }
}

impl Default for Quit {
    fn default() -> Quit {
        Quit::new("Leaving...")
    }
}

/// We have just connected to an IRC server.
#[derive(Debug, Clone, Message)]
pub struct Connected;

/// Send a private message.
#[derive(Debug, Clone)]
pub struct PrivateMessage {
    pub to: String,
    pub content: String,
}

impl Message for PrivateMessage {
    type Result = Result<(), IrcError>;
}

#[derive(Debug, Clone)]
pub struct Join {
    pub channels: String,
}

impl Message for Join {
    type Result = Result<(), IrcError>;
}

/// Identify the IRC client with the server, typically by sending a nick and
/// username.
#[derive(Debug, Clone)]
pub struct Identify;

impl Message for Identify {
    type Result = Result<(), IrcError>;
}

/// The server sent a *NOT REGISTERED* message.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct NotRegistered {
    pub args: Vec<String>,
    pub suffix: Option<String>,
}

/// Subscribe or unsubscribe to a particular message.
#[derive(Clone, Message)]
pub struct Registration<M>
where
    M: Message + Send + 'static,
    M::Result: Send,
{
    register: bool,
    recipient: Recipient<M>,
}

impl<M> Registration<M>
where
    M: Message + Clone + Send + 'static,
    M::Result: Send,
{
    pub fn new(recipient: Recipient<M>, register: bool) -> Registration<M> {
        Registration {
            recipient,
            register,
        }
    }

    pub fn register(recipient: Recipient<M>) -> Registration<M> {
        Registration::new(recipient, true)
    }

    pub fn unregister(recipient: Recipient<M>) -> Registration<M> {
        Registration::new(recipient, false)
    }

    pub fn for_actor<A>(addr: Addr<A>, register: bool) -> Registration<M>
    where
        A: Actor + Handler<M>,
        A::Context: ToEnvelope<A, M>,
    {
        Registration::new(addr.recipient(), register)
    }

    pub(crate) fn apply(self, message_box: &mut MessageBox) {
        let Registration {
            register,
            recipient,
        } = self;

        if register {
            message_box.register(recipient);
        } else {
            message_box.unregister(&recipient);
        }
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

/// Tell the IRC client to start listening for messages.
///
/// # Panic
///
/// This message can only be sent once. Telling the [`irc_bot::World`] to
/// [`StartListening`] multiple times will probably result in a panic.
#[derive(Debug, Copy, Clone, Message)]
pub struct StartListening;

/// Ask what channels we are currently listening to.
#[derive(Debug, Copy, Clone)]
pub struct Channels;

impl Message for Channels {
    type Result = HashMap<String, Addr<Channel>>;
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct PrivateMessageReceived {
    pub msg_target: String,
    pub content: String,
    pub raw: IrcMessage,
}
