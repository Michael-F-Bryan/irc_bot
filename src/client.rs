use actix::{
    Actor, Addr, Arbiter, Context, Handler, Message, Recipient, System,
};
use crate::logging::{Logger, Oops};
use crate::utils::MessageBox;
use failure::{Error, Fail};
use futures::future;
use irc::client::prelude::{
    Client as _Client, ClientExt, Config, Future, IrcClient, Stream,
};
use irc::client::ClientStream;
use std::panic::{self, AssertUnwindSafe};

/// A marker trait for all [`Message`] types which a bot can try to receive.
pub trait BotMessage: Message + Clone + Send + 'static {}

/// A connection to the IRC server.
pub struct Client {
    inner: IrcClient,
    logger: slog::Logger,
    error_logger: Addr<Logger>,
    message_box: MessageBox,
    msg_count: usize,
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
    F: FnOnce(&mut Client),
{
    let mut client = Client::connect(logger, error_logger.clone(), cfg)?;
    before_start(&mut client);
    let stream = client.stream();
    let client = client.start();

    let client_2 = client.clone();

    let fut = stream
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
    Arbiter::spawn(fut);

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
        })
    }

    fn stream(&self) -> ClientStream {
        self.inner.stream()
    }

    /// Register to receive a particular bot message type.
    pub fn register<M>(&mut self, recipient: Recipient<M>)
    where
        M: BotMessage,
        M::Result: Send,
    {
        self.message_box.register(recipient);
    }
}

impl Actor for Client {
    type Context = Context<Client>;
}

impl Handler<RawMessage> for Client {
    type Result = ();

    fn handle(
        &mut self,
        msg: RawMessage,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(self.logger, "Received a message";
            "message" => &format_args!("{}", msg.0));

        if self.msg_count == 0 {
            self.handle(NotifyBot(Connected), ctx);
        }

        self.msg_count += 1;
        self.handle(NotifyBot(msg), ctx);
    }
}

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

    fn handle(&mut self, msg: Quit, ctx: &mut Self::Context) {
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
    M: BotMessage,
    M::Result: Send,
{
    type Result = ();

    fn handle(&mut self, msg: NotifyBot<M>, _ctx: &mut Self::Context) {
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
