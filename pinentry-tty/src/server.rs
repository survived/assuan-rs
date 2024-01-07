use std::fmt;

use assuan_server::response::SecretData;
use either::Either;

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
        let pin_submitted = crate::ask_pin(
            &mut tty,
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

        let choice = crate::dialog(
            &mut tty,
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

#[derive(Debug)]
pub struct Error(Reason);

#[derive(Debug)]
enum Reason {
    OpenTty(std::io::Error),
    WriteTty(std::io::Error),
    ReadTty(std::io::Error),
    RawMode(std::io::Error),
    Dialog(crate::DialogError),
    OutputNotTty,
    PinTooLong,
    Internal(InternalError),
}

#[derive(Debug)]
enum InternalError {
    DebugInfoTooLong(assuan_server::response::TooLong),
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

impl assuan_server::HasErrorCode for Error {
    fn code(&self) -> assuan_server::ErrorCode {
        match self {
            Error(Reason::OpenTty(_)) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::WriteTty(_)) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::ReadTty(_)) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::RawMode(_)) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::Dialog(_)) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::OutputNotTty) => assuan_server::ErrorCode::ASS_GENERAL,
            Error(Reason::PinTooLong) => assuan_server::ErrorCode::TOO_LARGE,
            Error(Reason::Internal(_)) => assuan_server::ErrorCode::INTERNAL,
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

impl From<assuan_server::response::TooLong> for Error {
    fn from(err: assuan_server::response::TooLong) -> Self {
        Self(Reason::Internal(InternalError::DebugInfoTooLong(err)))
    }
}

impl From<crate::AskPinError> for Error {
    fn from(err: crate::AskPinError) -> Self {
        match err {
            crate::AskPinError::Read(err) => Error(Reason::ReadTty(err)),
            crate::AskPinError::Write(err) => Error(Reason::WriteTty(err)),
            crate::AskPinError::RawMode(err) => Error(Reason::RawMode(err)),
            crate::AskPinError::PinTooLong => Error(Reason::PinTooLong),
        }
    }
}

impl From<crate::DialogError> for Error {
    fn from(err: crate::DialogError) -> Self {
        match err {
            crate::DialogError::Read(err) => Error(Reason::ReadTty(err)),
            crate::DialogError::Write(err) => Error(Reason::WriteTty(err)),
            crate::DialogError::RawMode(err) => Error(Reason::RawMode(err)),
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
