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
        use std::io::Write as _;

        let (mut tty_in, mut tty_out) = self.open_tty()?;

        if let Some(error) = error {
            writeln!(tty_out, "Error: {error}").map_err(Error::WriteTty)?;
        }
        writeln!(tty_out, "{}", window_title).map_err(Error::WriteTty)?;
        if let Some(desc) = &desc {
            writeln!(tty_out, "{}", desc).map_err(Error::WriteTty)?;
        }
        writeln!(tty_out).map_err(Error::WriteTty)?;

        write!(tty_out, "{}", prompt).map_err(Error::WriteTty)?;
        tty_out.flush().map_err(Error::WriteTty)?;

        let Some(pin) = read_pin(&mut tty_in, &mut tty_out)? else {
            writeln!(tty_out, "Aborted.").map_err(Error::WriteTty)?;
            return Ok(None);
        };
        writeln!(tty_out).map_err(Error::WriteTty)?;

        Ok(Some(pin))
    }

    fn confirm(
        &mut self,
        error: Option<&str>,
        window_title: &str,
        desc: Option<&str>,
        buttons: pinentry::Buttons,
    ) -> Result<ConfirmAction, Self::Error> {
        use std::io::Write as _;

        let (mut tty_in, mut tty_out) = self.open_tty()?;

        if let Some(error) = error {
            writeln!(tty_out, "{error}").map_err(Error::WriteTty)?;
        }

        writeln!(tty_out, "{}", window_title).map_err(Error::WriteTty)?;
        if let Some(desc) = desc {
            writeln!(tty_out, "{}", desc).map_err(Error::WriteTty)?;
        }

        let mut options = heapless::Vec::<ConfirmOption<ConfirmAction>, 3>::new();

        options
            .push(ConfirmOption::new(buttons.ok, ConfirmAction::Ok))
            .expect("storage can fit exactly 3 elements");

        if let Some(not_ok) = buttons.not_ok {
            options
                .push(ConfirmOption::new(not_ok, ConfirmAction::NotOk))
                .expect("storage can fit exactly 3 elements")
        }
        if let Some(cancel) = buttons.cancel {
            options
                .push(ConfirmOption::new(cancel, ConfirmAction::Canceled))
                .expect("storage can fit exactly 3 elements")
        }

        let result = render_options(&mut tty_in, &mut tty_out, &options)
            .map(|x| *x.unwrap_or(&ConfirmAction::Canceled))
            .map_err(Error::ReadOption);
        writeln!(tty_out).map_err(Error::WriteTty)?;
        result
    }
}

fn read_pin(
    tty_in: &mut impl std::io::Read,
    tty_out: &mut (impl std::io::Write + std::os::fd::AsFd),
) -> Result<Option<SecretData>, Error> {
    use termion::{event::Key, input::TermRead, raw::IntoRawMode};

    let mut resp = SecretData::default();

    let _tty_out = tty_out.into_raw_mode().map_err(Error::RawMode)?;
    for k in tty_in.keys() {
        match k.map_err(Error::ReadPin)? {
            Key::Char('\n') | Key::Char('\r') => return Ok(Some(resp)),
            Key::Char(x) => {
                let mut s = [0u8; 4];
                let s = x.encode_utf8(&mut s);
                resp.append(s).map_err(|_| Error::PinTooLong)?;
            }
            Key::Backspace => {
                let _ = resp.pop();
            }
            Key::Ctrl('c')
            | Key::Ctrl('C')
            | Key::Ctrl('d')
            | Key::Ctrl('D')
            | Key::Null
            | Key::Esc => return Ok(None),
            _ => continue,
        }
    }
    todo!()
}

#[derive(Debug)]
struct ConfirmOption<'a, T> {
    text: &'a str,
    short: Option<char>,
    value: T,
}

impl<'a, T> ConfirmOption<'a, T> {
    pub fn new(text: &'a str, value: T) -> Self {
        let short = text.chars().find(|x| x.is_alphabetic());
        Self { text, short, value }
    }

    pub fn render(&self, tty_out: &mut impl std::io::Write) -> std::io::Result<()> {
        if let Some(short) = self.short {
            use termion::style::{NoUnderline, Underline};
            let (left, right) = self
                .text
                .split_once(short)
                .ok_or_else(|| std::io::Error::other("bug: `short` character not found"))?;
            write!(tty_out, "{left}{Underline}{short}{NoUnderline}{right}")?;
        } else {
            write!(tty_out, "{}", self.text)?;
        }
        Ok(())
    }
}

/// Renders options, asks user to choose one of them, returns whichever was chosen
fn render_options<'a, T>(
    tty_in: &mut impl std::io::Read,
    tty_out: &mut (impl std::io::Write + std::os::fd::AsFd),
    options: &'a [ConfirmOption<T>],
) -> std::io::Result<Option<&'a T>> {
    use termion::style::{NoUnderline, Underline};
    if options.len() > 9 {
        return Err(std::io::Error::other(
            "confirm dialog can not render more than 9 options",
        ));
    }
    for (i, option) in (1..).zip(options) {
        write!(tty_out, "  {Underline}{i}{NoUnderline} ")?;
        option.render(tty_out)?;
        writeln!(tty_out)?;
    }

    write!(tty_out, "Type [")?;
    for i in 1..=options.len() {
        write!(tty_out, "{i}")?;
    }
    for short in options
        .iter()
        .flat_map(|o| o.short)
        .map(|s| s.to_lowercase())
    {
        write!(tty_out, "{short}")?;
    }
    write!(tty_out, "] : ")?;
    tty_out.flush()?;

    use std::io::Write;
    use termion::{input::TermRead, raw::IntoRawMode};
    let mut tty_out = tty_out.into_raw_mode()?;

    for key in tty_in.events() {
        tty_out.flush()?;
        let termion::event::Event::Key(key) = key? else {
            continue;
        };
        match key {
            termion::event::Key::Char(x) => {
                if let Some(index) = x.to_digit(10) {
                    let Ok(index): Result<usize, _> = index.try_into() else {
                        continue;
                    };
                    let Some(index) = index.checked_sub(1) else {
                        continue;
                    };
                    let Some(option) = options.get(index) else {
                        continue;
                    };
                    write!(tty_out, "{}", x)?;
                    return Ok(Some(&option.value));
                } else {
                    let Some(option) = options.iter().find(|o| {
                        o.short
                            .map(|s| s.to_lowercase().eq(x.to_lowercase()))
                            .unwrap_or(false)
                    }) else {
                        continue;
                    };
                    write!(tty_out, "{}", x)?;
                    return Ok(Some(&option.value));
                }
            }
            termion::event::Key::Ctrl('c' | 'C' | 'd' | 'D') => {
                write!(tty_out, "Aborted.")?;
                return Ok(None);
            }
            _ => {
                // ignore
            }
        }
    }
    Ok(None)
}

impl PinentryTty {
    fn open_tty(
        &self,
    ) -> Result<(impl std::io::Read, impl std::io::Write + std::os::fd::AsFd), Error> {
        let tty_in = if let Some(path) = &self.tty {
            let fd = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(Error::OpenTty)?;
            if !termion::is_tty(&fd) {
                return Err(Error::OutputNotTty);
            }
            Either::Left(fd)
        } else {
            Either::Right(std::io::stdin())
        };

        let tty_out = if let Some(path) = &self.tty {
            let fd = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(Error::OpenTty)?;
            if !termion::is_tty(&fd) {
                return Err(Error::OutputNotTty);
            }
            TtyOut::File(fd)
        } else {
            TtyOut::Stdout(std::io::stdout())
        };

        Ok((tty_in, tty_out))
    }
}

/// `Either` was supposed to be used instead of `TtyOut` but `either` doesn't implement
/// `AsFd` trait
enum TtyOut {
    Stdout(std::io::Stdout),
    File(std::fs::File),
}
impl std::io::Write for TtyOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(buf),
            Self::File(file) => file.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            Self::File(file) => file.flush(),
        }
    }
}
impl std::os::fd::AsFd for TtyOut {
    fn as_fd(&self) -> std::os::fd::BorrowedFd {
        match self {
            Self::Stdout(stdout) => stdout.as_fd(),
            Self::File(file) => file.as_fd(),
        }
    }
}

#[derive(Debug)]
enum Error {
    OpenTty(std::io::Error),
    WriteTty(std::io::Error),
    ReadPin(std::io::Error),
    ReadOption(std::io::Error),
    RawMode(std::io::Error),
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
            Self::ReadPin(err) => write!(f, "read pin: {err}"),
            Self::ReadOption(err) => write!(f, "read option: {err}"),
            Self::RawMode(err) => write!(f, "enable raw mode: {err}"),
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
            Error::ReadPin(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::ReadOption(_) => assuan_server::ErrorCode::ASS_GENERAL,
            Error::RawMode(_) => assuan_server::ErrorCode::ASS_GENERAL,
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

fn main() {
    let mut server = pinentry::PinentryServer::new(PinentryTty::default()).build_assuan_server();

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    server.serve_client(stdin, stdout).unwrap();
}
