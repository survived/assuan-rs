use std::{fmt, io};

pub fn ask_pin(
    tty_in: &mut impl io::Read,
    tty_out: &mut (impl io::Write + std::os::fd::AsFd),
    prompt: impl fmt::Display,
    out: &mut impl PushPop<char>,
) -> Result<bool, AskPinError> {
    write!(tty_out, "{prompt}").map_err(AskPinError::Write)?;
    tty_out.flush().map_err(AskPinError::Write)?;

    if read_pin(tty_in, tty_out, out)? {
        writeln!(tty_out).map_err(AskPinError::Write)?;
        Ok(true)
    } else {
        writeln!(tty_out, "Aborted.").map_err(AskPinError::Write)?;
        Ok(false)
    }
}

fn read_pin(
    tty_in: &mut impl io::Read,
    tty_out: &mut (impl io::Write + std::os::fd::AsFd),
    out: &mut impl PushPop<char>,
) -> Result<bool, AskPinError> {
    use termion::{event::Key, input::TermRead, raw::IntoRawMode};

    let _tty_out = tty_out.into_raw_mode().map_err(AskPinError::RawMode)?;
    for k in tty_in.keys() {
        match k.map_err(AskPinError::Read)? {
            Key::Char('\n') | Key::Char('\r') => return Ok(true),
            Key::Char(x) => {
                out.push(x).map_err(|_| AskPinError::PinTooLong)?;
            }
            Key::Backspace => {
                let _ = out.pop();
            }
            Key::Ctrl('c')
            | Key::Ctrl('C')
            | Key::Ctrl('d')
            | Key::Ctrl('D')
            | Key::Null
            | Key::Esc => return Ok(false),
            _ => continue,
        }
    }
    Err(AskPinError::Read(io::ErrorKind::UnexpectedEof.into()))
}

pub trait PushPop<T> {
    fn push(&mut self, x: T) -> Result<(), T>;
    fn pop(&mut self) -> Option<T>;
}

impl PushPop<char> for assuan_server::response::SecretData {
    fn push(&mut self, x: char) -> Result<(), char> {
        (**self).push(x).map_err(|_| x)
    }

    fn pop(&mut self) -> Option<char> {
        (**self).pop()
    }
}

impl<T> PushPop<T> for Vec<T> {
    fn push(&mut self, x: T) -> Result<(), T> {
        Ok(self.push(x))
    }

    fn pop(&mut self) -> Option<T> {
        self.pop()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AskPinError {
    Cancelled,
    Read(io::Error),
    Write(io::Error),
    RawMode(io::Error),
    PinTooLong,
}

impl fmt::Display for AskPinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskPinError::Cancelled => write!(f, "operation cancelled"),
            AskPinError::Read(err) => write!(f, "read from tty: {err}"),
            AskPinError::Write(err) => write!(f, "write to tty: {err}"),
            AskPinError::RawMode(err) => write!(f, "switch to raw mode: {err}"),
            AskPinError::PinTooLong => write!(f, "pin is too long"),
        }
    }
}
