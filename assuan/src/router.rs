//! Routes requests between registered commands

use std::fmt;

pub use either::Either;

use crate::{ErrorCode, HasErrorCode, Response};

/// List of registered commands
pub trait CmdList<S> {
    /// Error type returned by [handle](Self::handle)
    type Error: fmt::Display + HasErrorCode;

    /// Routes the command execution
    ///
    /// Calling this function attempts to find a command `cmd` in the list. If it's present,
    /// the command handler function is called with `state` and `params` being the arguments,
    /// `Some(response)` is returned. If command is not found in the list, `None` is returned.
    fn handle(
        &mut self,
        cmd: &str,
        state: &mut S,
        params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>>;
}

/// Prepends a new command to the [list of commands](CmdList)
///
/// Not part of public API as it's a bit complex. [`AssuanServer::add_command`](crate::AssuanServer::add_command)
/// returns `impl CmdList<S>` in order to hide this type.
pub(crate) struct Cons<F, L> {
    cmd_name: &'static str,
    handler: F,
    tail: L,
}

impl<F, L> Cons<F, L> {
    /// Constructs a new [list of commands](CmdList) that has command with `name` and `handler`
    /// as the first element in the list, followed by a list `tail`
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

/// Empty [list of commands](CmdList)
pub struct Nil;

impl<S> CmdList<S> for Nil {
    type Error = std::convert::Infallible;

    /// Always returns `None`
    fn handle(
        &mut self,
        _cmd: &str,
        _state: &mut S,
        _params: Option<&str>,
    ) -> Option<Result<Response, Self::Error>> {
        None
    }
}

/// List of predefined commands
///
/// Contains commands:
/// * `BYE` that always responds with `OK` and terminates the connection
/// * `NOP` that always responds with `OK` and doesn't do anything else
pub struct PredefinedCmds<L = Nil> {
    tail: L,
}

impl Default for PredefinedCmds {
    fn default() -> Self {
        Self::new()
    }
}

impl PredefinedCmds {
    /// Constructs a list of predefined commands
    pub fn new() -> Self {
        Self::with_tail(Nil)
    }
}

impl<L> PredefinedCmds<L> {
    /// Constructs a list of predefined commands followed by `tail`
    pub fn with_tail(tail: L) -> Self {
        Self { tail }
    }
}

impl<S, L: CmdList<S>> CmdList<S> for PredefinedCmds<L> {
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
