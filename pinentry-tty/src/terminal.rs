//! Terminal abstraction
//!
//! This module provides [Terminal] trait that abstracts how `pinentry-tty`
//! interacts with the TTY terminal.
//!
//! [`Termion`] is a out-of-box terminal implementation provided when `termion` feature
//! is enabled (it is enabled by default).

use std::{fmt, io};

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
    /// Returns error if stdin/stdout do not correspond to TTY terminal (
    /// could be a case if program is piped)
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
