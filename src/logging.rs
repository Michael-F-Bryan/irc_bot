use actix::{Actor, Context, Handler, System};
use crate::utils::Panic;
use failure::Error;

/// An actor in charge of receiving log messages.
#[derive(Debug)]
pub struct Logger {
    inner: slog::Logger,
}

impl From<slog::Logger> for Logger {
    fn from(other: slog::Logger) -> Logger {
        Logger { inner: other }
    }
}

impl Actor for Logger {
    type Context = Context<Logger>;
}

#[derive(Message)]
pub struct Oops {
    pub error: Error,
    pub fatal: bool,
}

impl Oops {
    pub fn new<E: Into<Error>>(error: E) -> Oops {
        Oops {
            error: error.into(),
            fatal: false,
        }
    }

    pub fn fatal<E: Into<Error>>(error: E) -> Oops {
        Oops {
            error: error.into(),
            fatal: true,
        }
    }
}

impl From<Error> for Oops {
    fn from(other: Error) -> Oops {
        Oops::new(other)
    }
}

impl Handler<Oops> for Logger {
    type Result = ();

    fn handle(&mut self, msg: Oops, _ctx: &mut Context<Logger>) {
        error!(self.inner, "{}", msg.error.to_string(); 
            "fatal" => msg.fatal,
            "backtrace" => format_args!("{}", msg.error.backtrace()));

        if msg.fatal {
            info!(self.inner, "Aborting the application");
            System::current().stop();
        }
    }
}

impl Handler<Panic> for Logger {
    type Result = ();

    fn handle(&mut self, msg: Panic, _ctx: &mut Context<Logger>) {
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

        error!(self.inner, "A thread panicked";
            "message" => message,
            "file" => file,
            "line" => line,
            "column" => column,
            "thread" => thread,
            "backtrace" => bt);
    }
}
