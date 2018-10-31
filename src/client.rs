use actix::{
    Actor, Addr, Arbiter, AsyncContext, Context, Handler, Message, Recipient,
    System,
};
use crate::channel::Channel;
use crate::logging::{Logger, Oops};
use crate::utils::MessageBox;
use failure::{Error, Fail};
use futures::future;
use irc::client::prelude::{
    Client as _Client, ClientExt, Config, Future, IrcClient, Stream,
};
use irc::client::ClientStream;
use irc::proto::{Command, Response};
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};

/// A marker trait for all [`Message`] types which a bot can try to receive.
pub trait BotMessage: Message + Clone + Send + 'static {}

/// A connection to the IRC server.
#[derive(Debug)]
pub struct Client {
    inner: IrcClient,
    logger: slog::Logger,
    error_logger: Addr<Logger>,
    message_box: MessageBox,
    msg_count: usize,
    channels: HashMap<String, Addr<Channel>>,
}

/// Creates a new IRC client actor, making sure it will get sent any incoming
/// IRC messages.
pub fn spawn_client<F>(
    logger: slog::Logger,
    error_logger: Addr<Logger>,
    before_start: F,
    cfg: Config,
) -> Result<Addr<Client>, Error>
where
    F: FnOnce(&Addr<Client>, &Addr<Logger>, &mut Registration),
{
    let client = Client::connect(logger, error_logger.clone(), cfg)?;
    let stream = client.stream();
    let client = client.start();

    let mut reg = Registration(MessageBox::new());
    before_start(&client, &error_logger, &mut reg);
    let sending_registration = client.send(reg).map_err(|_| ());

    let client_2 = client.clone();

    let receive_messages = stream
        .map_err(Error::from)
        .and_then(move |msg| {
            client_2.send(RawMessage(msg)).map_err(Error::from)
        })
        .then(move |outcome| match outcome {
            Ok(_) => future::ok(()),
            Err(e) => {
                error_logger.do_send(Oops::new(e));
                future::ok(())
            }
        })
        .for_each(|_| future::ok(()));
    Arbiter::spawn(sending_registration.join(receive_messages).map(|_| ()));

    Ok(client)
}

impl Client {
    fn connect(
        logger: slog::Logger,
        error_logger: Addr<Logger>,
        cfg: Config,
    ) -> Result<Client, Error> {
        debug!(logger, "Started connecting");
        let inner = IrcClient::from_config(cfg)?;
        Ok(Client {
            inner,
            error_logger,
            logger,
            message_box: MessageBox::new(),
            msg_count: 0,
            channels: HashMap::default(),
        })
    }

    fn stream(&self) -> ClientStream {
        self.inner.stream()
    }
}

impl Actor for Client {
    type Context = Context<Client>;
}

#[derive(Message)]
pub struct Registration(MessageBox);

impl Registration {
    /// Register to receive a particular bot message type.
    pub fn register<M>(&mut self, recipient: Recipient<M>)
    where
        M: BotMessage,
        M::Result: Send,
    {
        self.0.register(recipient);
    }
}

impl Handler<Registration> for Client {
    type Result = ();

    fn handle(&mut self, msg: Registration, _ctx: &mut Self::Context) {
        self.message_box = msg.0;
    }
}

impl Handler<RawMessage> for Client {
    type Result = ();

    fn handle(
        &mut self,
        msg: RawMessage,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(self.logger, "Received a message";
            "prefix" => msg.0.prefix.as_ref(),
            "command" => format_args!("{:?}", msg.0.command));

        if self.msg_count == 0 {
            debug!(self.logger, "Notifying subscribers that we've connected");
            self.handle(NotifyBot(Connected), ctx);
        }

        self.msg_count += 1;

        if let Command::Response(
            Response::ERR_NOTREGISTERED,
            ref args,
            ref suffix,
        ) = msg.0.command
        {
            ctx.notify(NotifyBot(NotRegistered {
                args: args.clone(),
                suffix: suffix.clone(),
            }));
        }

        self.handle(NotifyBot(msg), ctx);
    }
}

/// The server sent a *NOT REGISTERED* message.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct NotRegistered {
    pub args: Vec<String>,
    pub suffix: Option<String>,
}

impl BotMessage for NotRegistered {}

/// A raw message as received from the IRC server.
#[derive(Debug, Clone, Message)]
pub struct RawMessage(pub irc::proto::Message);

impl BotMessage for RawMessage {}

/// Tell the IRC client to disconnect from the server and halt the actor system.
#[derive(Debug, Message)]
pub struct Quit {
    msg: String,
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

impl Handler<Quit> for Client {
    type Result = ();

    fn handle(&mut self, msg: Quit, _ctx: &mut Self::Context) {
        info!(self.logger, "Received a request to exit");

        if let Err(e) = self.inner.send_quit(msg.msg) {
            self.error_logger
                .do_send(Oops::fatal(e.context("Unable to quit")));
        }

        System::current().stop();
    }
}

/// Send a message to the bot.
#[derive(Debug, Clone)]
pub(crate) struct NotifyBot<M>(pub M);

impl<M> Message for NotifyBot<M> {
    type Result = ();
}

impl<M> Handler<NotifyBot<M>> for Client
where
    M: BotMessage + std::fmt::Debug,
    M::Result: Send,
{
    type Result = ();

    fn handle(&mut self, msg: NotifyBot<M>, _ctx: &mut Self::Context) {
        trace!(self.logger, "Passing a message on to the bot"; 
            "msg" => format_args!("{:?}", msg.0));

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            self.message_box.send(msg.0)
        }));

        if let Err(e) = result {
            let msg = crate::utils::panic_message(&e);
            self.error_logger.do_send(Oops::fatal(
                failure::err_msg(msg)
                    .context("Encountered a panic while handling a message"),
            ));
        }
    }
}

/// The [`Client`] has just connected to an IRC server.
#[derive(Debug, Clone, Message)]
pub struct Connected;

impl BotMessage for Connected {}

#[derive(Debug, Clone, Message)]
pub struct PrivateMessage {
    pub to: String,
    pub content: String,
}

impl Handler<PrivateMessage> for Client {
    type Result = ();

    fn handle(&mut self, msg: PrivateMessage, _ctx: &mut Self::Context) {
        debug!(self.logger, "Sending a private message";
            "recipient" => &msg.to,
            "content" => &msg.content);

        if let Err(e) = self.inner.send_privmsg(msg.to, msg.content) {
            self.error_logger.do_send(Oops::new(
                e.context("Unable to send a private message"),
            ));
        }
    }
}

#[derive(Debug, Clone, Message)]
pub struct Join {
    pub channels: String,
}

impl Handler<Join> for Client {
    type Result = ();

    fn handle(&mut self, msg: Join, _ctx: &mut Self::Context) {
        if let Err(e) = self.inner.send_join(msg.channels) {
            self.error_logger
                .do_send(Oops::new(e.context("Unable to join")));
        }
    }
}

#[derive(Debug, Clone, Message)]
pub struct Identify;

impl Handler<Identify> for Client {
    type Result = ();

    fn handle(&mut self, _msg: Identify, _ctx: &mut Self::Context) {
        info!(self.logger, "Sending identification");

        if let Err(e) = self.inner.identify() {
            self.error_logger
                .do_send(Oops::new(e.context("Unable to identify")));
        }
    }
}
