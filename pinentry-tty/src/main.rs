// TODO
// #![forbid(unused_crate_dependencies)]

use std::fmt;

use assuan_server::response::SecretData;
use either::Either;
use pinentry::ConfirmAction;

#[derive(Default)]
struct PinentryTty {
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
        let pin_submitted = pinentry_tty::ask_pin(
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
    ) -> Result<ConfirmAction, Self::Error> {
        let mut tty = self.open_tty()?;

        let mut options = Vec::with_capacity(3);
        options.push((buttons.ok, ConfirmAction::Ok));

        if let Some(not_ok) = buttons.not_ok {
            options.push((not_ok, ConfirmAction::NotOk));
        }
        if let Some(cancel) = buttons.cancel {
            options.push((cancel, ConfirmAction::Canceled));
        }

        let choice = pinentry_tty::dialog(
            &mut tty,
            &messages::Confirm {
                error,
                title: window_title,
                desc,
            },
            &options,
        )?;
        Ok(*choice.unwrap_or(&ConfirmAction::Canceled))
    }
}

impl PinentryTty {
    fn open_tty(&self) -> Result<impl pinentry_tty::Terminal, Error> {
        if let Some(path) = &self.tty {
            let tty_in = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(Error::OpenTty)?;
            let tty_out = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(Error::OpenTty)?;
            Ok(Either::Left(
                pinentry_tty::Termion::new(tty_in, tty_out).map_err(|_| Error::OutputNotTty)?,
            ))
        } else {
            Ok(Either::Right(
                pinentry_tty::Termion::new_stdio().map_err(|_| Error::OutputNotTty)?,
            ))
        }
    }
}

#[derive(Debug)]
enum Error {
    OpenTty(std::io::Error),
    WriteTty(std::io::Error),
    ReadTty(std::io::Error),
    RawMode(std::io::Error),
    AskPin(pinentry_tty::AskPinError),
    Dialog(pinentry_tty::DialogError),
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
            Self::OpenTty(err) => write!(f, "open tty: {err}"),
            Self::WriteTty(err) => write!(f, "write to tty: {err}"),
            Self::ReadTty(err) => write!(f, "read from tty: {err}"),
            Self::RawMode(err) => write!(f, "enable raw mode: {err}"),
            Self::AskPin(err) => write!(f, "get pin error: {err}"),
            Self::Dialog(err) => write!(f, "dialog error: {err}"),
            Self::OutputNotTty => write!(f, "output is not a tty"),
            Self::PinTooLong => write!(f, "pin is too long"),
            Self::Internal(err) => write!(f, "internal error: {err}"),
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
            Error::OpenTty(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::WriteTty(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::ReadTty(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::RawMode(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::AskPin(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::Dialog(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::OutputNotTty => assuan_server::ErrorCode::ASS_GENERAL,
            Error::PinTooLong => assuan_server::ErrorCode::TOO_LARGE,
            Error::Internal(_) => assuan_server::ErrorCode::INTERNAL,
        }
    }
}

impl From<InternalError> for Error {
    fn from(err: InternalError) -> Self {
        Error::Internal(err)
    }
}

impl From<assuan_server::response::TooLong> for Error {
    fn from(err: assuan_server::response::TooLong) -> Self {
        Self::Internal(InternalError::DebugInfoTooLong(err))
    }
}

impl From<pinentry_tty::AskPinError> for Error {
    fn from(err: pinentry_tty::AskPinError) -> Self {
        match err {
            pinentry_tty::AskPinError::Read(err) => Error::ReadTty(err),
            pinentry_tty::AskPinError::Write(err) => Error::WriteTty(err),
            pinentry_tty::AskPinError::RawMode(err) => Error::RawMode(err),
            pinentry_tty::AskPinError::PinTooLong => Error::PinTooLong,
            _ => Error::AskPin(err),
        }
    }
}

impl From<pinentry_tty::DialogError> for Error {
    fn from(err: pinentry_tty::DialogError) -> Self {
        match err {
            pinentry_tty::DialogError::Read(err) => Error::ReadTty(err),
            pinentry_tty::DialogError::Write(err) => Error::WriteTty(err),
            pinentry_tty::DialogError::RawMode(err) => Error::RawMode(err),
            _ => Error::Dialog(err),
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

fn main() {
    let mut server = pinentry::PinentryServer::new(PinentryTty::default()).build_assuan_server();

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    server.serve_client(stdin, stdout).unwrap();
}
