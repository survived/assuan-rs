//! Terminal abstraction
//!
//! This module provides [Terminal] trait that abstracts how `pinentry-tty`
//! interacts with the TTY terminal, and [`Tui`] commands defined for any
//! terminal.
//!
//! [`Termion`] is a out-of-box terminal implementation provided when `termion` feature
//! is enabled (it is enabled by default).

use std::{fmt, io};

use crate::PushPop;

/// TTY terminal
///
/// Terminal is required to implement [Read](io::Read) and [Write](io::Write) traits.
/// It should recognize ANSI control sequences.
pub trait Terminal: io::Read + io::Write {
    /// Returns iterator over keys pressed by the terminal user and a writer that can be
    /// used to write something to the terminal
    ///
    /// Characters must not appear in the user terminal. It's also required that even special
    /// keys like backspace and esc are captured. Usually, it means that terminal needs to be
    /// switched into a [raw mode](termion::raw). Writer should hold a guard for raw mode:
    /// when writer is dropped, the original state of the terminal must be restored. For that
    /// reason, writer must outlive the iterator over keys.
    fn keys(
        &mut self,
    ) -> io::Result<(
        impl Iterator<Item = io::Result<Key>> + '_,
        impl io::Write + '_,
    )>;
}

/// Pinentry TUI commands implemented for any [`Terminal`]
pub trait Tui: Terminal {
    /// Asks user to provide a PIN
    ///
    /// Similar to [`crate::ask_pin`] but defined for generic [`Terminal`] and returns more verbose [`AskPinError`]
    fn ask_pin(
        &mut self,
        prompt: impl fmt::Display,
        out: &mut impl PushPop<char>,
    ) -> Result<bool, AskPinError>;
    /// Asks user to choose among one or several options
    ///
    /// Similar to [`crate::dialog`] but defined for generic [`Terminal`] and returns more verbose [`DialogError`]
    fn dialog<'a, T>(
        &mut self,
        message: impl fmt::Display,
        options: &'a [(&str, T)],
    ) -> Result<Option<&'a T>, DialogError>;
}

impl<L, R> Terminal for either::Either<L, R>
where
    L: Terminal,
    R: Terminal,
{
    fn keys(
        &mut self,
    ) -> io::Result<(
        impl Iterator<Item = io::Result<Key>> + '_,
        impl io::Write + '_,
    )> {
        use either::{Left, Right};
        match self {
            Left(tty) => {
                let (keys, tty_out) = tty.keys()?;
                Ok((Left(keys), Left(tty_out)))
            }
            Right(tty) => {
                let (keys, tty_out) = tty.keys()?;
                Ok((Right(keys), Right(tty_out)))
            }
        }
    }
}

/// Key pressed by terminal user
pub enum Key {
    /// User pressed a regular key represented by the char
    Char(char),
    /// User pressed a key while holding a Ctrl button
    Ctrl(char),
    /// User sent null signal
    Null,
    /// User pressed escape button
    Esc,
    /// User pressed backspace button
    Backspace,
}

/// Default terminal implementation based on [termion] crate
#[cfg(feature = "termion")]
pub struct Termion<I, O> {
    input: I,
    output: O,
}

#[cfg(feature = "termion")]
impl<I, O> Termion<I, O>
where
    I: io::Read + std::os::fd::AsFd,
    O: io::Write + std::os::fd::AsFd,
{
    /// Constructs a terminal from given input and output that can be used
    /// to read and to write to the terminal
    ///
    /// Returns error if input or output are not a tty terminal
    pub fn new(input: I, output: O) -> Result<Self, NotTty> {
        if !termion::is_tty(&input.as_fd()) || !termion::is_tty(&output.as_fd()) {
            Err(NotTty)
        } else {
            Ok(Self { input, output })
        }
    }
}

#[cfg(feature = "termion")]
impl Termion<std::io::Stdin, std::io::Stdout> {
    /// Constructs a terminal from stdin and stdout
    ///
    /// Returns error if stdin/stdout do not correspond to TTY terminal
    /// (could be the case if program is piped)
    pub fn new_stdio() -> Result<Self, NotTty> {
        Self::new(std::io::stdin(), std::io::stdout())
    }
}

#[cfg(feature = "termion")]
impl<I, O> io::Read for Termion<I, O>
where
    I: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

#[cfg(feature = "termion")]
impl<I, O> io::Write for Termion<I, O>
where
    O: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

#[cfg(feature = "termion")]
impl<I, O> Terminal for Termion<I, O>
where
    I: io::Read,
    O: io::Write + std::os::fd::AsFd,
{
    fn keys(
        &mut self,
    ) -> io::Result<(
        impl Iterator<Item = io::Result<Key>> + '_,
        impl io::Write + '_,
    )> {
        use termion::input::TermRead;
        use termion::raw::IntoRawMode;
        let output = (&mut self.output).into_raw_mode()?;

        let input_keys = (&mut self.input).keys().flat_map(|key| match key {
            Ok(termion::event::Key::Char(x)) => Some(Ok(Key::Char(x))),
            Ok(termion::event::Key::Ctrl(x)) => Some(Ok(Key::Ctrl(x))),
            Ok(termion::event::Key::Null) => Some(Ok(Key::Null)),
            Ok(termion::event::Key::Esc) => Some(Ok(Key::Esc)),
            Ok(termion::event::Key::Backspace) => Some(Ok(Key::Backspace)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        });

        Ok((input_keys, output))
    }
}

/// Provided input/output do not correspond to a TTY terminal
#[derive(Debug)]
pub struct NotTty;

impl fmt::Display for NotTty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a tty")
    }
}

impl std::error::Error for NotTty {}

impl From<NotTty> for io::Error {
    fn from(err: NotTty) -> Self {
        io::Error::new(io::ErrorKind::Unsupported, err)
    }
}

impl<T: Terminal> Tui for T {
    fn ask_pin(
        &mut self,
        prompt: impl fmt::Display,
        out: &mut impl PushPop<char>,
    ) -> Result<bool, AskPinError> {
        write!(self, "{prompt}").map_err(AskPinError::Write)?;
        self.flush().map_err(AskPinError::Write)?;

        if read_pin(self, out)? {
            writeln!(self).map_err(AskPinError::Write)?;
            Ok(true)
        } else {
            writeln!(self, "Aborted.").map_err(AskPinError::Write)?;
            Ok(false)
        }
    }

    fn dialog<'a, O>(
        &mut self,
        message: impl fmt::Display,
        options: &'a [(&str, O)],
    ) -> Result<Option<&'a O>, DialogError> {
        if options.is_empty() {
            return Err(DialogError::TooFewOptions);
        }
        let options = options.iter().fold(
            Vec::with_capacity(options.len()),
            |mut acc, (text, value)| {
                let option = DialogOption::new(text, value, &acc);
                acc.push(option);
                acc
            },
        );

        writeln!(self, "{message}").map_err(DialogError::Write)?;

        let result = render_options(self, &options);
        writeln!(self).map_err(DialogError::Write)?;
        result
    }
}

fn read_pin(tty: &mut impl Terminal, out: &mut impl PushPop<char>) -> Result<bool, AskPinError> {
    let (keys, _tty_out) = tty.keys().map_err(AskPinError::RawMode)?;
    for k in keys {
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

/// Explains why [`ask_pin`](Tui::ask_pin) failed
#[derive(Debug)]
#[non_exhaustive]
pub enum AskPinError {
    /// Error occurred while reading input from the user
    Read(io::Error),
    /// Error occurred while writing to TTY
    Write(io::Error),
    /// Could not switch TTY into raw mode
    RawMode(io::Error),
    /// User entered too long PIN
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

impl std::error::Error for AskPinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AskPinError::Read(err) => Some(err),
            AskPinError::Write(err) => Some(err),
            AskPinError::RawMode(err) => Some(err),
            AskPinError::PinTooLong => None,
        }
    }
}

impl From<AskPinError> for io::Error {
    fn from(err: AskPinError) -> Self {
        let kind = match &err {
            AskPinError::Read(err) | AskPinError::Write(err) | AskPinError::RawMode(err) => {
                err.kind()
            }
            AskPinError::PinTooLong => io::ErrorKind::Other,
        };
        io::Error::new(kind, err)
    }
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
            use ctrl_seq::{NoUnderline, Underline};
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
    tty: &mut impl Terminal,
    options: &[DialogOption<'a, T>],
) -> Result<Option<&'a T>, DialogError> {
    use ctrl_seq::{NoUnderline, Underline};
    use std::io::Write;

    if options.len() > 9 {
        return Err(DialogError::TooManyOptions);
    }

    for (i, option) in (1..).zip(options) {
        write!(tty, "  {Underline}{i}{NoUnderline} ").map_err(DialogError::Write)?;
        option.render(tty)?;
        writeln!(tty).map_err(DialogError::Write)?;
    }

    write!(tty, "Type [").map_err(DialogError::Write)?;
    for i in 1..=options.len() {
        write!(tty, "{i}").map_err(DialogError::Write)?;
    }
    for short in options
        .iter()
        .flat_map(|o| o.short)
        .map(|s| s.to_lowercase())
    {
        write!(tty, "{short}").map_err(DialogError::Write)?;
    }
    write!(tty, "] : ").map_err(DialogError::Write)?;
    tty.flush().map_err(DialogError::Write)?;

    let (keys, mut tty_out) = tty.keys().map_err(DialogError::RawMode)?;

    for key in keys {
        tty_out.flush().map_err(DialogError::Write)?;
        match key.map_err(DialogError::Read)? {
            Key::Char(x) => {
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
            Key::Ctrl('c' | 'C' | 'd' | 'D') | Key::Null | Key::Esc => {
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

/// Explains why [`dialog`](Tui::dialog) failed
#[derive(Debug)]
#[non_exhaustive]
pub enum DialogError {
    /// Error occurred while reading input from the user
    Read(io::Error),
    /// Error occurred while writing to TTY
    Write(io::Error),
    /// Could not switch TTY into raw mode
    RawMode(io::Error),
    /// No options were provided as input: at least one option is required
    TooManyOptions,
    /// Too many options were provided as input: [`dialog`](Tui::dialog) can take no more than 9 options
    TooFewOptions,
    /// Bug occurred
    Bug(Bug),
}

/// Error indicating that a bug occurred
///
/// If you encounter this error, please file an issue!
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
            DialogError::TooFewOptions => write!(
                f,
                "invalid arguments: at least one option must be specified"
            ),
            DialogError::Bug(Bug(BugReason::ShortCharacterNotFound)) => {
                write!(f, "bug occurred: short character not found")
            }
        }
    }
}

impl std::error::Error for DialogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DialogError::Read(err) => Some(err),
            DialogError::Write(err) => Some(err),
            DialogError::RawMode(err) => Some(err),
            DialogError::TooManyOptions | DialogError::TooFewOptions | DialogError::Bug(_) => None,
        }
    }
}

impl From<DialogError> for io::Error {
    fn from(err: DialogError) -> Self {
        let kind = match &err {
            DialogError::Read(err) | DialogError::Write(err) | DialogError::RawMode(err) => {
                err.kind()
            }
            DialogError::TooManyOptions | DialogError::TooFewOptions => io::ErrorKind::InvalidInput,
            DialogError::Bug(_) => io::ErrorKind::Other,
        };
        io::Error::new(kind, err)
    }
}

mod ctrl_seq {
    use std::fmt;

    /// Create a CSI-introduced sequence.
    macro_rules! csi {
        ($( $l:expr ),*) => { concat!("\x1B[", $( $l ),*) };
    }

    /// Derive a CSI sequence struct.
    macro_rules! derive_csi_sequence {
        ($doc:expr, $name:ident, $value:expr) => {
            #[doc = $doc]
            #[derive(Copy, Clone)]
            pub struct $name;

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, csi!($value))
                }
            }

            impl AsRef<[u8]> for $name {
                fn as_ref(&self) -> &'static [u8] {
                    csi!($value).as_bytes()
                }
            }

            impl AsRef<str> for $name {
                fn as_ref(&self) -> &'static str {
                    csi!($value)
                }
            }
        };
    }

    derive_csi_sequence!("Underlined text.", Underline, "4m");
    derive_csi_sequence!("Undo underlined text.", NoUnderline, "24m");
}
