use actix::Addr;
use crate::logging::Logger;
use failure::Backtrace;
use std::panic::{self, PanicInfo};
use std::thread;

/// A RAII guard which will forward any panics to the logging actor.
pub struct PanicHook {
    previous_handler: Option<Box<dyn Fn(&PanicInfo) + 'static + Sync + Send>>,
}

impl PanicHook {
    pub fn new(logger: Addr<Logger>) -> PanicHook {
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

impl<'a> From<&'a PanicInfo<'a>> for Panic {
    fn from(other: &'a PanicInfo) -> Panic {
        let backtrace = Backtrace::new();
        let thread = thread::current().name().map(String::from);

        let message = if let Some(msg) = other.payload().downcast_ref::<&str>()
        {
            msg.to_string()
        } else if let Some(msg) = other.payload().downcast_ref::<String>() {
            msg.clone()
        } else {
            String::from("Panic")
        };

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
