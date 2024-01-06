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
        self.push(x);
        Ok(())
    }

    fn pop(&mut self) -> Option<T> {
        self.pop()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AskPinError {
    Read(io::Error),
    Write(io::Error),
    RawMode(io::Error),
    PinTooLong,
}

impl fmt::Display for AskPinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskPinError::Read(err) => write!(f, "read from tty: {err}"),
            AskPinError::Write(err) => write!(f, "write to tty: {err}"),
            AskPinError::RawMode(err) => write!(f, "switch to raw mode: {err}"),
            AskPinError::PinTooLong => write!(f, "pin is too long"),
        }
    }
}

pub fn dialog<'a, T>(
    tty_in: &mut impl io::Read,
    tty_out: &mut (impl io::Write + std::os::fd::AsFd),
    message: impl fmt::Display,
    options: &'a [(&str, T)],
) -> Result<Option<&'a T>, DialogError> {
    let options = options.iter().fold(
        Vec::with_capacity(options.len()),
        |mut acc, (text, value)| {
            let option = DialogOption::new(text, value, &acc);
            acc.push(option);
            acc
        },
    );

    writeln!(tty_out, "{message}").map_err(DialogError::Write)?;

    let result = render_options(tty_in, tty_out, &options);
    writeln!(tty_out).map_err(DialogError::Write)?;
    result
}

struct DialogOption<'a, T> {
    text: &'a str,
    short: Option<char>,
    value: &'a T,
}

impl<'a, T> DialogOption<'a, T> {
    pub fn new(text: &'a str, value: &'a T, existing_options: &[DialogOption<T>]) -> Self {
        let short_already_used =
            |&short: &char| existing_options.iter().any(|o| Some(short) == o.short);
        let available_short = text
            .chars()
            .filter(|x| x.is_alphabetic())
            .find(|x| !short_already_used(x));

        Self {
            short: available_short,
            text,
            value,
        }
    }

    pub fn render(&self, tty_out: &mut impl std::io::Write) -> Result<(), DialogError> {
        if let Some(short) = self.short {
            use termion::style::{NoUnderline, Underline};
            let (left, right) = self
                .text
                .split_once(short)
                .ok_or(BugReason::ShortCharacterNotFound)?;
            write!(tty_out, "{left}{Underline}{short}{NoUnderline}{right}")
                .map_err(DialogError::Write)?;
        } else {
            write!(tty_out, "{}", self.text).map_err(DialogError::Write)?;
        }
        Ok(())
    }
}

fn render_options<'a, T>(
    tty_in: &mut impl io::Read,
    tty_out: &mut (impl io::Write + std::os::fd::AsFd),
    options: &[DialogOption<'a, T>],
) -> Result<Option<&'a T>, DialogError> {
    use termion::style::{NoUnderline, Underline};
    if options.len() > 9 {
        return Err(DialogError::TooManyOptions);
    }

    for (i, option) in (1..).zip(options) {
        write!(tty_out, "  {Underline}{i}{NoUnderline} ").map_err(DialogError::Write)?;
        option.render(tty_out)?;
        writeln!(tty_out).map_err(DialogError::Write)?;
    }

    write!(tty_out, "Type [").map_err(DialogError::Write)?;
    for i in 1..=options.len() {
        write!(tty_out, "{i}").map_err(DialogError::Write)?;
    }
    for short in options
        .iter()
        .flat_map(|o| o.short)
        .map(|s| s.to_lowercase())
    {
        write!(tty_out, "{short}").map_err(DialogError::Write)?;
    }
    write!(tty_out, "] : ").map_err(DialogError::Write)?;
    tty_out.flush().map_err(DialogError::Write)?;

    use termion::{input::TermRead, raw::IntoRawMode};
    let mut tty_out = tty_out.into_raw_mode().map_err(DialogError::RawMode)?;

    for key in tty_in.keys() {
        tty_out.flush().map_err(DialogError::Write)?;
        match key.map_err(DialogError::Read)? {
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
                    write!(tty_out, "{}", x).map_err(DialogError::Write)?;
                    return Ok(Some(option.value));
                } else {
                    let Some(option) = options.iter().find(|o| {
                        o.short
                            .map(|s| s.to_lowercase().eq(x.to_lowercase()))
                            .unwrap_or(false)
                    }) else {
                        continue;
                    };
                    write!(tty_out, "{}", x).map_err(DialogError::Write)?;
                    return Ok(Some(option.value));
                }
            }
            termion::event::Key::Ctrl('c' | 'C' | 'd' | 'D') => {
                write!(tty_out, "Aborted.").map_err(DialogError::Write)?;
                return Ok(None);
            }
            _ => {
                // ignore
            }
        }
    }
    Ok(None)
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DialogError {
    Read(io::Error),
    Write(io::Error),
    RawMode(io::Error),
    TooManyOptions,
    Bug(Bug),
}

#[derive(Debug)]
pub struct Bug(BugReason);

#[derive(Debug)]
enum BugReason {
    ShortCharacterNotFound,
}

impl From<BugReason> for DialogError {
    fn from(err: BugReason) -> Self {
        DialogError::Bug(Bug(err))
    }
}

impl fmt::Display for DialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialogError::Read(err) => write!(f, "read from tty: {err}"),
            DialogError::Write(err) => write!(f, "write to tty: {err}"),
            DialogError::RawMode(err) => write!(f, "switch to raw mode: {err}"),
            DialogError::TooManyOptions => write!(f, "invalid arguments: too many options"),
            DialogError::Bug(Bug(BugReason::ShortCharacterNotFound)) => {
                write!(f, "bug occurred: short character not found")
            }
        }
    }
}
