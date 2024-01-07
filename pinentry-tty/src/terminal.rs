use std::{fmt, io};

pub trait Terminal: io::Read + io::Write {
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

pub enum Key {
    Char(char),
    Ctrl(char),
    Null,
    Esc,
    Backspace,
}

pub struct Termion<I, O> {
    input: I,
    output: O,
}

impl<I, O> Termion<I, O>
where
    I: io::Read + std::os::fd::AsFd,
    O: io::Write + std::os::fd::AsFd,
{
    pub fn new(input: I, output: O) -> Result<Self, NotTty> {
        if !termion::is_tty(&input.as_fd()) || !termion::is_tty(&output.as_fd()) {
            Err(NotTty)
        } else {
            Ok(Self { input, output })
        }
    }
}

impl Termion<std::io::Stdin, std::io::Stdout> {
    pub fn new_stdio() -> Result<Self, NotTty> {
        Self::new(std::io::stdin(), std::io::stdout())
    }
}

impl<I, O> io::Read for Termion<I, O>
where
    I: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

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

#[derive(Debug)]
pub struct NotTty;

impl fmt::Display for NotTty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a tty")
    }
}
