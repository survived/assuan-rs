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

pub struct AssuanServer<S, L> {
    service: S,
    cmd_handlers: L,
}

impl<S> AssuanServer<S, router::SystemCmds> {
    /// Constructs a new assuan server
    ///
    /// Server has some [predefined commands](router::SystemCmds). You may construct a server
    /// without them by using [`AssuanServer::without_system_cmds`].
    ///
    /// Commands can be registered via [.add_command](AssuanServer::add_command) method.
    pub fn new(service: S) -> Self {
        Self {
            service,
            cmd_handlers: router::SystemCmds::new(),
        }
    }
}

impl<S> AssuanServer<S, router::Nil> {
    pub fn without_system_cmds(service: S) -> Self {
        Self {
            service,
            cmd_handlers: router::Nil,
        }
    }
}

impl<S, L: router::CmdList<S>> AssuanServer<S, L> {
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

    pub fn serve_client<C>(&mut self, conn: &mut C) -> io::Result<()>
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
        .append("ERR ")?
        .append(&code.0.to_string())?
        .append(" ")?
        .append(desc.as_ref())
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
