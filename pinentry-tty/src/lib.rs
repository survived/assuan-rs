//! This crate provides basic interactions with terminal users such as asking a PIN and showing a dialog
//!
//! Library is focused on security to treat sensitive data such as PIN appropriately.
//!
//! Two fundamental TUI interactions provided are:
//! 1. [`ask_pin`] to ask user to provide a PIN
//! 2. [`dialog`] to ask user to choose one of available options
//!
//! Initially, these functions were developed to replace [`pinentry-tty` utility][pinentry],
//! but generally they can be used in any application. When `server` feature is enabled,
//! [`server`](server()) function is available that can be used to launch pinentry-tty server.
//!
//! [pinentry]: https://git.gnupg.org/cgi-bin/gitweb.cgi?p=pinentry.git
#![forbid(unused_crate_dependencies)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use std::{fmt, io};

#[cfg(feature = "termion")]
pub use terminal::Termion;
pub use terminal::{Terminal, Tui};

pub use zeroize;

#[cfg(feature = "server")]
pub mod server;
pub mod terminal;

/// Builds Assuan server that implements a pinentry-tty tool
///
/// Alias for wrapping [server::PinentryTty] into [pinentry::PinentryServer] and
/// converting into [assuan::AssuanServer].
///
/// ### Example
/// Launch a pinentry-tty server that accepts commands from stdin and writes responses
/// to stdout:
/// ```rust
#[doc = include_str!("main.rs")]
/// ```
#[cfg(feature = "server")]
pub fn server() -> assuan::AssuanServer<
    pinentry::PinentryServer<server::PinentryTty>,
    impl assuan::router::CmdList<pinentry::PinentryServer<server::PinentryTty>>,
> {
    pinentry::PinentryServer::new(server::PinentryTty::default()).build_assuan_server()
}

/// Asks user to provide a PIN
///
/// Prints the `prompt` to stdout and reads a PIN from the user from stdin. Characters that user
/// types will not be visible in the terminal. Writes PIN into `out`. `out` is expected to be empty.
///
/// When user types a newline (a.k.a. Enter), indicating end of input, `Ok(true)` is returned.
/// If `Ctrl-C`, `Ctrl-D` or `Escape` are pressed, `Ok(false)` is returned.
///
/// Returns error if:
/// * Stdin/stdout are not a TTY (could be the case if program is piped)
/// * User entered PIN exceeding `out` capacity
/// * Any I/O error occurred in the process
///
/// ## Example
/// ```rust,no_run
/// use pinentry_tty::zeroize::Zeroizing;
///
/// // Note: if user types a PIN overflowing string capacity, an error is returned
/// let mut pin = Zeroizing::new(String::with_capacity(100));
/// pinentry_tty::ask_pin("PIN: ", &mut pin)?;
/// # Ok::<_, std::io::Error>(())
/// ```
///
/// User will see a prompt:
/// ```text
/// PIN:
/// ```
/// and then user will be able to type a PIN. Characters of the PIN will not be visible.
/// PIN can be submitted by typing Enter, or aborted by typing `Ctrl-C`, `Ctrl-D` or `Escape`.
///
/// ## Generic terminals
/// This function is tied to [`termion` backend](Termion) and stdin/stdout. [`Tui::ask_pin`] can be used
/// with any [`Terminal`]
#[cfg(feature = "termion")]
pub fn ask_pin(prompt: impl fmt::Display, out: &mut impl PushPop<char>) -> io::Result<bool> {
    let mut tty = Termion::new_stdio()?;
    Ok(tty.ask_pin(prompt, out)?)
}

/// Asks user to choose among one or several options
///
/// Prints a message and available options to user, then waits until user chooses
/// one of them or aborts the dialog.
///
/// This can be used to implement various interactions with the user. A dialog with
/// one option can be used to display an informational alert to confirm that user saw
/// the message. A dialog with two options could be asking for confirmation for some
/// action, and so on.
///
/// User can choose an option by typing a single character. This character can be either
/// numerical or alphabetical:
/// * Each option is numbered so user can choose any option by
///   typing it sequential number from `1` to `9`.
/// * Option can be chosen by typing its first alphabetical character. For instance:
///   * If two options are given: `Continue` and `Abort`, then user can type `C` (uppercase
///     or lowercase) to <u>c</u>ontinue, and type `A` to <u>a</u>bort.
///   * If given options `Continue` and `Cancel`, user can type `C` to <u>c</u>ontinue or
///     `A` to c<u>a</u>ncel.
///
/// ## Number of options
/// At least one option must be provided. There cannot be more than 9 options.
/// Otherwise an error is returned.
///
/// ## Returns
/// * `Ok(Some(chosen_option))` if user's chosen an option
/// * `Ok(None)` if user aborted the dialog (e.g. by pressing `Ctrl-C`)
/// * `Err(err)` if:
///   * `options` list is empty
///   * `options` list has more than 9 elements
///   * Any I/O error occurred in the process
///
/// ## Example
/// ```rust,no_run
#[doc = include_str!("../examples/do_you_want_to_proceed.rs")]
/// ```
///
/// User will see:
///
/// > Do you want to proceed? \
/// > &nbsp;  <u>1</u> <u>Y</u>es \
/// > &nbsp;  <u>2</u> <u>N</u>o \
/// > &nbsp;Type \[12yn\] :
///
/// * Typing 1 or `Y` (uppercase or lowercase) returns `Ok(Some(&Choice::Yes))`
/// * Typing 2 or `N` returns `Ok(Some(&Choice::No))`
/// * Typing `Ctrl-C`, `Ctrl-D` or `Escape` aborts the dialog and returns `Ok(None)`
/// * `Err(err)` is returned if any error occurs
///
/// You can try it out via `cargo run --example do_you_want_to_proceed`.
///
/// ## Generic terminals
/// This function is tied to [`termion` backend](Termion) and stdin/stdout. [`Tui::dialog`] can be used
/// with any [`Terminal`]
#[cfg(feature = "termion")]
pub fn dialog<'a, T>(
    message: impl fmt::Display,
    options: &'a [(&str, T)],
) -> io::Result<Option<&'a T>> {
    let mut tty = Termion::new_stdio()?;
    Ok(tty.dialog(message, options)?)
}

/// Container that provides push/pop access
///
/// The trait is used to store PIN typed by the user in [`ask_pin`], therefore the trait implementation
/// must treat its content as highly sensitive.
///
/// Out of box, we provide an implementation of the trait for the `Zeroizing<String>`:
/// 1. [`Zeroizing`](zeroize::Zeroizing) ensures that the PIN is erased from the memory when dropped
/// 2. Implementation does not allow the string to grow: `push` operation is only possible
///    if the string has some capacity left \
///    Growing the string leaves a partial copy of it on heap which is not desired for sensitive information.
///
/// ## Example
/// ```rust
/// use pinentry_tty::PushPop;
/// use zeroize::Zeroizing;
///
/// let mut buffer = Zeroizing::new(String::with_capacity(10));
/// for x in "0123456789".chars() {
///     buffer.push(x)?;
/// }
///
/// // Pushing any more character would require string to grow, so error is returned
/// buffer.push('a').unwrap_err();
/// # Ok::<_, char>(())
/// ```
pub trait PushPop<T> {
    /// Appends `x`
    ///
    /// Returns `Err(x)` if container cannot take it
    fn push(&mut self, x: T) -> Result<(), T>;
    /// Pops the last element
    fn pop(&mut self) -> Option<T>;
}

#[cfg(feature = "server")]
impl PushPop<char> for assuan::response::SecretData {
    fn push(&mut self, x: char) -> Result<(), char> {
        (**self).push(x).map_err(|_| x)
    }

    fn pop(&mut self) -> Option<char> {
        (**self).pop()
    }
}

/// Push/pop access to the string without reallocation
///
/// `push` operation will never cause the internal buffer of `String` to grow
impl PushPop<char> for zeroize::Zeroizing<String> {
    /// Appends a character to the string if it has free capacity
    ///
    /// ```rust
    /// use pinentry_tty::PushPop;
    /// use zeroize::Zeroizing;
    ///
    /// let mut buf = Zeroizing::new(String::with_capacity(2));
    /// buf.push('a').unwrap();
    /// buf.push('b').unwrap();
    ///
    /// // String has no internal capacity left. Pushing new element
    /// // will not succeed
    /// buf.push('c').unwrap_err();
    /// ```
    fn push(&mut self, x: char) -> Result<(), char> {
        if self.len() + x.len_utf8() <= self.capacity() {
            (**self).push(x);
            Ok(())
        } else {
            Err(x)
        }
    }

    fn pop(&mut self) -> Option<char> {
        (**self).pop()
    }
}
