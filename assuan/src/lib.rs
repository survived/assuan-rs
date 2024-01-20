//! Assuan server
//!
//! This crate helps implementing inter-process communication (IPC) servers based on [Assuan protocol]
//! which is mainly used in GPG software. One reason someone would want to use this library is to replace
//! components of GPG ecosystem with pure Rust implementation.
//!
//! As an example, here's a typical communication between pinentry server `S` (that's used to ask user
//! a pin) and client `C` (most commonly, GPG software that performs some cryptographic operation):
//! ```text
//! S: OK ready
//! C: SETDESC Please provide your PIN for decryption key
//! S: OK success
//! C: SETPROMPT PIN:
//! S: OK success
//! C: GETPIN
//! S: D 1234
//! S: OK success
//! C: BYE
//! S: OK success
//! ```
//!
//! This crate takes the most boilerplate from implementing the server, namely:
//! * Percent-encoding and decoding certain characters of requests and responses
//! * Enforcing limitations set by the assuan spec, such as the [max line size](MAX_LINE_SIZE)
//! * Understanding which command is being called by the client and invoking appropriate method
//! * Zeroizing responses in memory that contain sensitive data
//! * Handling common assuan commands such as `BYE` and `NOP`
//!
//! ### Minimal example
//! ```rust
#![doc = include_str!("../examples/greeter.rs")]
//! ```
//!
//! You can check it out via:
//! ```bash
//! cargo run --example greeter
//! ```
//!
//! Example of using it:
//! ```text
//! S: OK how can I serve you?
//! C: GREET Bob
//! S: D Hello, Bob! My name's Alice
//! S: OK success
//! C: BYE
//! S: OK success
//! ```
//!
//! [Assuan protocol]: https://www.gnupg.org/documentation/manuals/assuan/index.html

#![forbid(unused_crate_dependencies)]
#![deny(missing_docs)]

use core::fmt;
use std::io;

use response::ResponseLine;

use self::line_reader::LineReader;

pub use self::{
    error_code::{ErrorCode, HasErrorCode, WithErrorCode},
    response::Response,
};

mod error_code;
mod line_reader;
mod percent_decode;
pub mod response;
pub mod router;

/// Maximum size of a line following the assuan specs
pub const MAX_LINE_SIZE: usize = 1000;

/// Assuan Server
///
/// Wraps the server state provided [at construction](Self::new). When serves a client in
/// [`AssuanServer::serve_client`], it routes incoming requests between commands registered
/// via [`AssuanServer::add_command`]. Out-of-box, it recognizes some
/// [predefined commands](router::PredefinedCmds) like `BYE` (can be disabled by using
/// [`AssuanServer::without_predefined_cmds`]).
pub struct AssuanServer<S, L> {
    service: S,
    cmd_handlers: L,
}

impl<S> AssuanServer<S, router::PredefinedCmds> {
    /// Constructs a new assuan server
    ///
    /// Server has some [predefined commands](router::PredefinedCmds). You may construct a server
    /// without them by using [`AssuanServer::without_predefined_cmds`].
    ///
    /// Commands can be registered via [.add_command](AssuanServer::add_command) method.
    pub fn new(service: S) -> Self {
        Self {
            service,
            cmd_handlers: router::PredefinedCmds::new(),
        }
    }
}

impl<S> AssuanServer<S, router::Nil> {
    /// Constructs a new assuan server without any [predefined commands](router::PredefinedCmds)
    pub fn without_predefined_cmds(service: S) -> Self {
        Self {
            service,
            cmd_handlers: router::Nil,
        }
    }
}

impl<S, L: router::CmdList<S>> AssuanServer<S, L> {
    /// Registers a new command
    ///
    /// Takes register-sensitive `cmd_name` and a `handler` that will actually process incoming
    /// requests.
    pub fn add_command<E>(
        self,
        cmd_name: &'static str,
        handler: impl FnMut(&mut S, Option<&str>) -> Result<Response, E>,
    ) -> AssuanServer<S, impl router::CmdList<S>>
    where
        E: fmt::Display + HasErrorCode,
    {
        AssuanServer {
            service: self.service,
            cmd_handlers: router::Cons::new(cmd_name, handler, self.cmd_handlers),
        }
    }

    /// Serves a client: reads the requests from `read` and writes the responses to `write`
    ///
    /// Incoming requests will be routed between registered commands
    pub fn serve_client<R, W>(&mut self, read: R, write: W) -> io::Result<()>
    where
        R: io::Read,
        W: io::Write,
    {
        self.serve_client_conn(&mut Conn { read, write })
    }

    /// Server a client: reads the requests and writes the responses to `conn`
    pub fn serve_client_conn<C>(&mut self, conn: &mut C) -> io::Result<()>
    where
        C: io::Read + io::Write,
    {
        // Greet client
        conn.write_all(b"OK how can I serve you?\n")?;

        fn write_error(out: &mut impl io::Write, code: ErrorCode, desc: &str) -> io::Result<()> {
            let resp = error(code, desc).map_err(|_err| io::Error::other("error is too long"))?;
            resp.write(out)
        }

        // Serve client's requests
        loop {
            match self.serve_request(conn) {
                Ok(true) => continue,
                Ok(false) => break,
                Err(ServeError::MalformedUtf8(err)) => {
                    return write_error(conn, ErrorCode::ASS_INV_VALUE, &err.to_string())
                }
                Err(ServeError::MalformedPercentEncoding) => {
                    return write_error(
                        conn,
                        ErrorCode::ASS_PARAMETER,
                        "malformed percent encoding",
                    )
                }
                Err(ServeError::ErrorTooLong(_err)) => {
                    return write_error(conn, ErrorCode::INTERNAL, "error is too long")
                }
                Err(ServeError::Read(err)) => {
                    return write_error(conn, ErrorCode::ASS_READ_ERROR, &err.to_string())
                }
                Err(ServeError::Write(err)) => {
                    // we can't really send error to the client as write call already resulted
                    // into error
                    return Err(err);
                }
                Err(ServeError::ReceivedLineTooLong) => {
                    return write_error(conn, ErrorCode::ASS_LINE_TOO_LONG, "line is too long")
                }
            }
        }

        Ok(())
    }

    fn serve_request<C>(&mut self, conn: &mut C) -> Result<bool, ServeError>
    where
        C: io::Read + io::Write,
    {
        // Receive a line from the client
        let mut line_reader = LineReader::new();
        let Some(line) = line_reader.read_line(conn)? else {
            return Ok(false);
        };

        // Line must be a valid UTF-8 string
        let line = std::str::from_utf8(line).map_err(ServeError::MalformedUtf8)?;

        if line.starts_with('#') || line.is_empty() {
            // Lines beginning with a # or empty lines are ignored
            return Ok(true);
        }

        // Parse command
        let (cmd, args) = line
            .split_once(' ')
            .map(|(cmd, args)| (cmd, Some(args)))
            .unwrap_or_else(|| (line, None));

        // Decode percent encoding of args
        let args = args
            .map(|args| percent_decode::percent_decode(args).collect::<Result<String, _>>())
            .transpose()
            .map_err(|_| ServeError::MalformedPercentEncoding)?;
        let args = args.as_deref();

        // Route and execute the command
        let response = self.cmd_handlers.handle(cmd, &mut self.service, args);

        // Convert error to string
        let response = response.map(|resp| resp.map_err(|err| (err.code(), err.to_string())));
        let response = response
            .as_ref()
            .map(|resp| resp.as_ref().map_err(|(code, desc)| (*code, desc.as_str())));

        // Handle `unknown command` error
        let response = response.unwrap_or(Err((ErrorCode::ASS_UNKNOWN_CMD, "Unknown command")));

        match response {
            Ok(resp) => {
                resp.write(conn).map_err(ServeError::Write)?;
                Ok(!resp.connection_needs_be_closed())
            }
            Err((code, err)) => {
                let resp = error(code, err).map_err(ServeError::ErrorTooLong)?;
                resp.write(conn).map_err(ServeError::Write)?;
                Ok(true)
            }
        }
    }
}

fn error(code: ErrorCode, desc: impl AsRef<str>) -> Result<ResponseLine, response::TooLong> {
    response::ResponseLine::new()
        .chain("ERR ")?
        .chain(&code.0.to_string())?
        .chain(" ")?
        .chain(desc.as_ref())
}

enum ServeError {
    MalformedUtf8(std::str::Utf8Error),
    MalformedPercentEncoding,
    ErrorTooLong(response::TooLong),
    Read(io::Error),
    Write(io::Error),
    ReceivedLineTooLong,
}

impl From<line_reader::ReadLineError> for ServeError {
    fn from(err: line_reader::ReadLineError) -> Self {
        match err {
            line_reader::ReadLineError::Read(err) => Self::Read(err),
            line_reader::ReadLineError::LineTooLong => Self::ReceivedLineTooLong,
        }
    }
}

struct Conn<R, W> {
    read: R,
    write: W,
}

impl<R: io::Read, W> io::Read for Conn<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}

impl<R, W: io::Write> io::Write for Conn<R, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}
