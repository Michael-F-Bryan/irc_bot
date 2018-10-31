use actix::{Actor, Context};

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
}

impl Actor for Channel {
    type Context = Context<Channel>;
}
