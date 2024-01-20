//! Helps implementing a pinentry server
//!
//! This crate provides a [`PinentryServer`] that takes the most boilerplate of implementing
//! pinentry server, requiring only to implement the [core operations](PinentryCmds) defining
//! how to ask user for [PIN](PinentryCmds::get_pin) and for [confirmation](PinentryCmds::confirm)

#![forbid(unused_crate_dependencies)]
#![deny(missing_docs)]

use core::fmt;

#[doc(no_inline)]
pub use assuan::{
    self,
    response::{Response, SecretData},
    HasErrorCode,
};

/// Pinentry server
///
/// Wraps a minimalistic [`PinentryCmds` trait](PinentryCmds) that tells how actually PIN should be
/// obtained from the user, and provides implementation of fully-functional pinentry server that
/// follows the Assuan protocol, receives and recognizes the commands, and so on.
pub struct PinentryServer<S: PinentryCmds> {
    cmds: S,

    desc: Option<String>,
    prompt: Option<String>,
    window_title: Option<String>,

    button_ok: Option<String>,
    button_not_ok: Option<String>,
    button_cancel: Option<String>,

    error_text: Option<String>,
}

/// Buttons that should be displayed in [confirmation dialog](PinentryCmds::confirm)
pub struct Buttons<'a> {
    /// OK button, suggesting user to give consent
    pub ok: &'a str,
    /// Not OK button, suggesting user to refuse to whatever is asked
    pub not_ok: Option<&'a str>,
    /// Cancel button, suggesting user to abort the operation
    pub cancel: Option<&'a str>,
}

/// The core of pinentry server: [retrieving pin](Self::get_pin) from the user, and showing the
/// [confirmation prompt](Self::confirm)
///
/// [`PinentryServer`] requires this commands to be defined by the library user in order to provide
/// a fully functional pinentry server.
pub trait PinentryCmds {
    /// Error returned by the commands
    type Error: HasErrorCode + fmt::Display;

    /// Tells that pinentry was asked to use the given TTY
    fn set_tty(&mut self, path: std::path::PathBuf) -> Result<(), Self::Error>;

    /// Asks user to enter PIN
    ///
    /// # Inputs
    /// * `error` is `Some(_)` if some message containing error description needs to be displayed to
    ///   user before prompting PIN
    /// * `window_title` is suggested title of the window
    /// * `desc`, if present, contains more detailed information of why and/or what for PIN is required
    /// * `prompt` is short text that should be displayed right before to where PIN in entered
    ///
    /// # Outputs
    /// * `Ok(Some(pin))` if user entered a pin
    /// * `Ok(None)` if user aborted the prompt (e.g. pressed `Ctrl-C` or closed the window)
    /// * `Err(err)` if any unexpected error occurred

    fn get_pin(
        &mut self,
        error: Option<&str>,
        window_title: &str,
        desc: Option<&str>,
        prompt: &str,
    ) -> Result<Option<SecretData>, Self::Error>;

    /// Asks user to confirm action
    ///
    /// # Inputs
    /// * `error` is `Some(_)` if some message containing error description needs to be displayed to
    ///   user before asking for confirmation
    /// * `window_title` is suggested title of the window
    /// * `desc`, if present, contains more detailed information of what to be confirmed
    /// * `buttons` are the buttons that should be prompted to the user
    ///
    /// # Outputs
    /// Function should return whichever `button` user pressed. For instance, if [`buttons.ok`](Buttons::ok)
    /// was pressed, [`ConfirmChoice::Ok`] should be returned). If user aborted the confirmation (e.g. by
    /// pressing `Ctrl-C` or closing the window), [`ConfirmChoice::Canceled`] should be returned.
    fn confirm(
        &mut self,
        error: Option<&str>,
        window_title: &str,
        desc: Option<&str>,
        buttons: Buttons,
    ) -> Result<ConfirmChoice, Self::Error>;
}

/// Choice of the user in [confirm dialog](PinentryCmds::confirm)
#[derive(Debug, Clone, Copy)]
pub enum ConfirmChoice {
    /// User gave consent
    Ok,
    /// User aborted the operation
    Canceled,
    /// User refused to whatever was asked
    NotOk,
}

macro_rules! define_setters {
    ($($setter_fn:ident $var:ident $($modify:expr)?),*$(,)?) => {$(
        fn $setter_fn(&mut self, $var: Option<&str>) -> Result<Response, HandleError<S::Error>> {
            self.$var = $var.map(str::to_string);
            $(
                if let Some(var) = &mut self.$var {
                    #[allow(clippy::redundant_closure_call)]
                    let _: () = $modify(var);
                }
            )?
            Ok(Response::ok())
        }
    )*};
}

impl<S: PinentryCmds> PinentryServer<S> {
    /// Constructs a pinentry server
    pub fn new(cmds: S) -> Self {
        Self {
            cmds,
            desc: None,
            prompt: None,
            window_title: None,
            button_ok: None,
            button_not_ok: None,
            button_cancel: None,
            error_text: None,
        }
    }

    /// Builds an assuan server ready to serve requests from the client
    pub fn build_assuan_server(
        self,
    ) -> assuan::AssuanServer<Self, impl assuan::router::CmdList<Self>> {
        assuan::AssuanServer::new(self)
            .add_command("OPTION", Self::option)
            .add_command("SETTIMEOUT", Self::not_currently_supported)
            .add_command("SETDESC", Self::set_desc)
            .add_command("SETPROMPT", Self::set_prompt)
            .add_command("SETTITLE", Self::set_window_title)
            .add_command("SETOK", Self::set_button_ok)
            .add_command("SETCANCEL", Self::set_button_cancel)
            .add_command("SETNOTOK", Self::set_button_not_ok)
            .add_command("SETERROR", Self::set_error_text)
            .add_command("SETQUALITYBAR", Self::not_currently_supported)
            .add_command("SETQUALITYBAR_TT", Self::not_currently_supported)
            .add_command("GETPIN", Self::get_pin)
            .add_command("CONFIRM", Self::confirm)
            .add_command("MESSAGE", Self::message)
    }

    fn get_pin(&mut self, _args: Option<&str>) -> Result<Response, HandleError<S::Error>> {
        self.cmds
            .get_pin(
                self.error_text.as_deref(),
                self.window_title
                    .as_ref()
                    .map(String::as_ref)
                    .unwrap_or("Enter PIN"),
                self.desc.as_deref(),
                self.prompt.as_deref().unwrap_or("PIN: "),
            )
            .map_err(HandleError::PinentryCmd)?
            .ok_or(HandleError::NoPin)
            .map(Into::into)
    }

    fn _confirm(&mut self, one_button: bool) -> Result<Response, HandleError<S::Error>> {
        let buttons = if one_button {
            Buttons {
                ok: self.button_ok.as_deref().unwrap_or("OK"),
                not_ok: None,
                cancel: None,
            }
        } else {
            let mut btns = Buttons {
                ok: self.button_ok.as_deref().unwrap_or("OK"),
                not_ok: self.button_not_ok.as_ref().map(String::as_ref),
                cancel: self.button_cancel.as_ref().map(String::as_ref),
            };
            if btns.not_ok.is_none() && btns.cancel.is_none() {
                btns.cancel = Some("Cancel");
            }
            btns
        };
        let response = self
            .cmds
            .confirm(
                self.error_text.as_deref(),
                self.window_title.as_deref().unwrap_or("Confirm"),
                self.desc.as_ref().map(String::as_ref),
                buttons,
            )
            .map_err(HandleError::PinentryCmd)?;
        match response {
            ConfirmChoice::Ok => Ok(Response::ok()),
            ConfirmChoice::NotOk => Err(HandleError::ConfirmRefused),
            ConfirmChoice::Canceled => Err(HandleError::ConfirmCancelled),
        }
    }

    fn confirm(&mut self, args: Option<&str>) -> Result<Response, HandleError<S::Error>> {
        let one_button = args
            .map(|args| args.trim() == "--one-button")
            .unwrap_or(false);
        self._confirm(one_button)
    }

    fn message(&mut self, _args: Option<&str>) -> Result<Response, HandleError<S::Error>> {
        self._confirm(true)
    }

    fn option(&mut self, args: Option<&str>) -> Result<Response, HandleError<S::Error>> {
        let Some(args) = args else {
            return Ok(Response::ok_with_debug_info("ignored, no args")?);
        };

        let (var, value) = args.split_once([' ', '=']).unwrap_or((args, ""));

        match var {
            "ttyname" => {
                self.cmds
                    .set_tty(value.into())
                    .map_err(HandleError::PinentryCmd)?;

                Ok(Response::ok())
            }
            _ => Ok(Response::ok_with_debug_info("unknown option, ignored")?),
        }
    }

    fn not_currently_supported(
        &mut self,
        _args: Option<&str>,
    ) -> Result<Response, HandleError<S::Error>> {
        Ok(Response::ok_with_debug_info(
            "not currently supported, ignored",
        )?)
    }

    define_setters! {
        set_desc desc,
        set_prompt prompt |prompt: &mut String| if !prompt.ends_with(' ') { prompt.push(' ') },
        set_window_title window_title,
        set_button_ok button_ok,
        set_button_not_ok button_not_ok,
        set_button_cancel button_cancel,
        set_error_text error_text,
    }
}

#[derive(Debug)]
enum HandleError<E> {
    DebugInfoTooLong(assuan::response::TooLong),
    ConfirmRefused,
    ConfirmCancelled,
    NoPin,
    PinentryCmd(E),
}

impl<E: fmt::Display> fmt::Display for HandleError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DebugInfoTooLong(_) => write!(f, "internal error: debug info is too long"),
            Self::ConfirmRefused => write!(f, "refused"),
            Self::ConfirmCancelled => write!(f, "canceled"),
            Self::NoPin => write!(f, "no pin given"),
            Self::PinentryCmd(err) => err.fmt(f),
        }
    }
}

impl<E: HasErrorCode> HasErrorCode for HandleError<E> {
    fn code(&self) -> assuan::ErrorCode {
        match self {
            HandleError::DebugInfoTooLong(_) => assuan::ErrorCode::INTERNAL,
            HandleError::ConfirmRefused => assuan::ErrorCode::NOT_CONFIRMED,
            HandleError::ConfirmCancelled => assuan::ErrorCode::CANCELED,
            HandleError::NoPin => assuan::ErrorCode::NO_PIN,
            HandleError::PinentryCmd(err) => err.code(),
        }
    }
}

impl<E> From<assuan::response::TooLong> for HandleError<E> {
    fn from(err: assuan::response::TooLong) -> Self {
        Self::DebugInfoTooLong(err)
    }
}
