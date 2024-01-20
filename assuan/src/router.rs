use std::fmt;

pub use either::Either;

use crate::{ErrorCode, HasErrorCode, Response};

pub trait CmdList<S> {
    type Error: fmt::Display + HasErrorCode;

    fn handle(
        &mut self,
        cmd: &str,
        state: &mut S,
        params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>>;
}

pub(crate) struct Cons<F, L> {
    cmd_name: &'static str,
    handler: F,
    tail: L,
}

impl<F, L> Cons<F, L> {
    pub fn new(name: &'static str, handler: F, tail: L) -> Self {
        Self {
            cmd_name: name,
            handler,
            tail,
        }
    }
}

impl<F, S, E, L> CmdList<S> for Cons<F, L>
where
    F: FnMut(&mut S, Option<&str>) -> Result<Response, E>,
    L: CmdList<S>,
    E: fmt::Display + HasErrorCode,
{
    type Error = Either<E, L::Error>;

    fn handle(
        &mut self,
        cmd: &str,
        state: &mut S,
        params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>> {
        if cmd == self.cmd_name {
            Some((self.handler)(state, params).map_err(Either::Left))
        } else {
            self.tail
                .handle(cmd, state, params)
                .map(|result| result.map_err(Either::Right))
        }
    }
}

pub struct Nil;

impl<S> CmdList<S> for Nil {
    type Error = std::convert::Infallible;

    fn handle(
        &mut self,
        _cmd: &str,
        _state: &mut S,
        _params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>> {
        None
    }
}

pub struct SystemCmds<L = Nil> {
    tail: L,
}

impl Default for SystemCmds {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemCmds {
    pub fn new() -> Self {
        Self::with_tail(Nil)
    }
}

impl<L> SystemCmds<L> {
    pub fn with_tail(tail: L) -> Self {
        Self { tail }
    }
}

impl<S, L: CmdList<S>> CmdList<S> for SystemCmds<L> {
    type Error = L::Error;

    fn handle(
        &mut self,
        cmd: &str,
        state: &mut S,
        params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>> {
        use crate::response;
        match cmd {
            "NOP" => {
                // No operation. Returns OK without any action.
                Some(Ok(response::Ok::new().into()))
            }
            "BYE" => {
                // Close the connection. The server will respond with OK.
                Some(Ok(response::Ok::new().close_connection(true).into()))
            }
            _ => {
                // It is not a system command
                self.tail.handle(cmd, state, params)
            }
        }
    }
}

impl<L, R> HasErrorCode for Either<L, R>
where
    L: HasErrorCode,
    R: HasErrorCode,
{
    fn code(&self) -> ErrorCode {
        match self {
            Either::Left(v) => v.code(),
            Either::Right(v) => v.code(),
        }
    }
}
