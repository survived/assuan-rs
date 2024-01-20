//! Pinentry TTY server
//!
//! Note: it's easier to use [`pinentry_tty::server()`](crate::server()) function.

use std::fmt;

use assuan::response::SecretData;
use either::Either;

use crate::terminal::Tui;

/// [PinentryCmds](pinentry::PinentryCmds) implementation based on [`ask_pin`](crate::ask_pin)
/// and [`dialog`](crate::dialog) functions provided by this library
///
/// Can be converted into assuan server using [`pinentry::PinentryServer`]
#[derive(Default)]
pub struct PinentryTty {
    tty: Option<std::path::PathBuf>,
}

impl pinentry::PinentryCmds for PinentryTty {
    type Error = Error;

    fn set_tty(&mut self, path: std::path::PathBuf) -> Result<(), Self::Error> {
        self.tty = Some(path);
        Ok(())
    }

    fn get_pin(
        &mut self,
        error: Option<&str>,
        window_title: &str,
        desc: Option<&str>,
        prompt: &str,
    ) -> Result<Option<SecretData>, Self::Error> {
        let mut tty = self.open_tty()?;

        let mut pin = SecretData::default();
        let pin_submitted = tty.ask_pin(
            &messages::PinPrompt {
                error,
                title: window_title,
                desc,
                prompt,
            },
            &mut pin,
        )?;

        Ok(Some(pin).filter(|_| pin_submitted))
    }

    fn confirm(
        &mut self,
        error: Option<&str>,
        window_title: &str,
        desc: Option<&str>,
        buttons: pinentry::Buttons,
    ) -> Result<pinentry::ConfirmAction, Self::Error> {
        let mut tty = self.open_tty()?;

        let mut options = Vec::with_capacity(3);
        options.push((buttons.ok, pinentry::ConfirmAction::Ok));

        if let Some(not_ok) = buttons.not_ok {
            options.push((not_ok, pinentry::ConfirmAction::NotOk));
        }
        if let Some(cancel) = buttons.cancel {
            options.push((cancel, pinentry::ConfirmAction::Canceled));
        }

        let choice = tty.dialog(
            &messages::Confirm {
                error,
                title: window_title,
                desc,
            },
            &options,
        )?;
        Ok(*choice.unwrap_or(&pinentry::ConfirmAction::Canceled))
    }
}

impl PinentryTty {
    fn open_tty(&self) -> Result<impl crate::Terminal, Error> {
        if let Some(path) = &self.tty {
            let tty_in = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(Reason::OpenTty)?;
            let tty_out = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(Reason::OpenTty)?;
            Ok(Either::Left(
                crate::Termion::new(tty_in, tty_out).map_err(|_| Reason::OutputNotTty)?,
            ))
        } else {
            Ok(Either::Right(
                crate::Termion::new_stdio().map_err(|_| Reason::OutputNotTty)?,
            ))
        }
    }
}

/// Error returned by [PinentryTty]
#[derive(Debug)]
pub struct Error(Reason);

#[derive(Debug)]
enum Reason {
    OpenTty(std::io::Error),
    WriteTty(std::io::Error),
    ReadTty(std::io::Error),
    RawMode(std::io::Error),
    Dialog(crate::terminal::DialogError),
    OutputNotTty,
    PinTooLong,
    Internal(InternalError),
}

#[derive(Debug)]
enum InternalError {
    DebugInfoTooLong(assuan::response::TooLong),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self(Reason::OpenTty(err)) => write!(f, "open tty: {err}"),
            Self(Reason::WriteTty(err)) => write!(f, "write to tty: {err}"),
            Self(Reason::ReadTty(err)) => write!(f, "read from tty: {err}"),
            Self(Reason::RawMode(err)) => write!(f, "enable raw mode: {err}"),
            Self(Reason::Dialog(err)) => write!(f, "dialog error: {err}"),
            Self(Reason::OutputNotTty) => write!(f, "output is not a tty"),
            Self(Reason::PinTooLong) => write!(f, "pin is too long"),
            Self(Reason::Internal(err)) => write!(f, "internal error: {err}"),
        }
    }
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DebugInfoTooLong(_) => write!(f, "debug info is too long"),
        }
    }
}

impl assuan::HasErrorCode for Error {
    fn code(&self) -> assuan::ErrorCode {
        match self {
            Error(Reason::OpenTty(_)) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::WriteTty(_)) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::ReadTty(_)) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::RawMode(_)) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::Dialog(_)) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::OutputNotTty) => assuan::ErrorCode::ASS_GENERAL,
            Error(Reason::PinTooLong) => assuan::ErrorCode::TOO_LARGE,
            Error(Reason::Internal(_)) => assuan::ErrorCode::INTERNAL,
        }
    }
}

impl From<Reason> for Error {
    fn from(err: Reason) -> Self {
        Error(err)
    }
}

impl From<InternalError> for Error {
    fn from(err: InternalError) -> Self {
        Error(Reason::Internal(err))
    }
}

impl From<assuan::response::TooLong> for Error {
    fn from(err: assuan::response::TooLong) -> Self {
        Self(Reason::Internal(InternalError::DebugInfoTooLong(err)))
    }
}

impl From<crate::terminal::AskPinError> for Error {
    fn from(err: crate::terminal::AskPinError) -> Self {
        match err {
            crate::terminal::AskPinError::Read(err) => Error(Reason::ReadTty(err)),
            crate::terminal::AskPinError::Write(err) => Error(Reason::WriteTty(err)),
            crate::terminal::AskPinError::RawMode(err) => Error(Reason::RawMode(err)),
            crate::terminal::AskPinError::PinTooLong => Error(Reason::PinTooLong),
        }
    }
}

impl From<crate::terminal::DialogError> for Error {
    fn from(err: crate::terminal::DialogError) -> Self {
        match err {
            crate::terminal::DialogError::Read(err) => Error(Reason::ReadTty(err)),
            crate::terminal::DialogError::Write(err) => Error(Reason::WriteTty(err)),
            crate::terminal::DialogError::RawMode(err) => Error(Reason::RawMode(err)),
            _ => Error(Reason::Dialog(err)),
        }
    }
}

mod messages {
    use std::fmt;

    pub struct PinPrompt<'a> {
        pub error: Option<&'a str>,
        pub title: &'a str,
        pub desc: Option<&'a str>,
        pub prompt: &'a str,
    }

    impl<'a> fmt::Display for PinPrompt<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(error) = self.error {
                writeln!(f, "Error: {error}")?;
            }
            writeln!(f, "{}", self.title)?;
            if let Some(desc) = self.desc {
                writeln!(f, "{desc}")?;
            }
            writeln!(f)?;

            write!(f, "{}", self.prompt)
        }
    }

    pub struct Confirm<'a> {
        pub error: Option<&'a str>,
        pub title: &'a str,
        pub desc: Option<&'a str>,
    }

    impl<'a> fmt::Display for Confirm<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(error) = self.error {
                writeln!(f, "Error: {error}")?;
            }
            writeln!(f, "{}", self.title)?;
            if let Some(desc) = self.desc {
                writeln!(f, "{desc}")?;
            }
            Ok(())
        }
    }
}
