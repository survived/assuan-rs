//! Response of assuan server

/// Assuan server successful response
///
/// Any response indicating success of requested operation. Responses
/// indicating error should be constructed by returning `Err(_)` in
/// request handler
#[allow(clippy::large_enum_variant)]
pub enum Response {
    /// Secret data response
    SecretData(SecretData),
    /// Data response
    Data(Data),
    /// OK response
    Ok(Ok),
}

impl From<SecretData> for Response {
    fn from(v: SecretData) -> Self {
        Response::SecretData(v)
    }
}

impl From<Data> for Response {
    fn from(v: Data) -> Self {
        Response::Data(v)
    }
}

impl From<Ok> for Response {
    fn from(v: Ok) -> Self {
        Response::Ok(v)
    }
}

impl Response {
    /// Constructs a default OK response
    ///
    /// Alias to:
    /// ```rust
    /// use assuan_server::response::{Response, Ok};
    ///
    /// let r: Response = Ok::new().into();
    /// ```
    pub fn ok() -> Self {
        Self::Ok(Ok::new())
    }

    /// Constructs an OK response with custom debug info
    ///
    /// Alias to:
    /// ```rust
    /// use assuan_server::response::{Response, Ok};
    ///
    /// let r: Response = Ok::with_debug_info("custom debug info")?.into();
    /// # Ok::<_, assuan_server::response::TooLong>(())
    /// ```
    pub fn ok_with_debug_info(info: &str) -> Result<Self, TooLong> {
        Ok::with_debug_info(info).map(Self::Ok)
    }

    /// Constructs a data response
    ///
    /// Alias to:
    /// ```rust
    /// use assuan_server::response::{Response, Data};
    ///
    /// let r: Response = Data::new("data to be sent")?.into();
    /// # Ok::<_, assuan_server::response::TooLong>(())
    /// ```
    pub fn data(data: &str) -> Result<Self, TooLong> {
        Data::new(data).map(Self::Data)
    }

    pub(crate) fn write(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        match self {
            Self::Ok(ok) => ok.resp.write(out),
            Self::Data(data) => {
                data.data_resp.write(out)?;
                data.ok.resp.write(out)
            }
            Self::SecretData(data) => {
                data.data_resp.write(out)?;
                data.ok.resp.write(out)
            }
        }
    }

    /// Indicates whether a connection needs to be closed when response is sent
    pub fn connection_needs_be_closed(&self) -> bool {
        match self {
            Self::Ok(r) => r.close_conn,
            Self::Data(r) => r.ok.close_conn,
            Self::SecretData(r) => r.ok.close_conn,
        }
    }
}

/// [Data] response containing sensitive information
///
/// For security purposes, sensitive data is allocated on heap and zeroized on drop.
///
/// Use [Default] trait to construct an empty data response, and then [`append`](Data::append) function to add actual
/// data to the response.
///
/// ### Example
/// ```rust
/// use assuan_server::response::SecretData;
///
/// let mut response = SecretData::default();
/// response.append("my password")?;
/// # Ok::<_, assuan_server::response::TooLong>(())
/// ```
pub type SecretData = Box<zeroize::Zeroizing<Data>>;

/// Data response
///
/// On a wire, data response has format:
///
/// ```text
/// D [escaped data]\n
/// OK success\n
/// ```
///
/// Data is UTF8 string. Certain characters in the string are percent-encoded (e.g. `\n` is transmitted as `%A0`).
/// Percent encoding is done automatically when data is written. Data string is limited by [Data::MAX_BYTES] size
/// in bytes after percent-encoding.
///
/// Data response is always followed by [Ok] response. By default, `OK success` is sent, however, custom debug
/// info may be specified via [Data::with_custom_ok] or [Data::with_debug_info]. Assuan protocol also allows
/// data responses to be followed by `ERR` response, but the library doesn't support that.
#[derive(Clone, Copy)]
pub struct Data {
    data_resp: ResponseLine,
    ok: Ok,
}

impl Data {
    /// Max size of data response as specified in assuan spec
    ///
    /// Assuan spec sets the limit for max response size: 1000 bytes. 3 bytes of those are
    /// used for data prefix (`"D "` of 2 bytes) and final `\n` byte indicating end of the
    /// response. So the response data may be up to 997 bytes long.
    pub const MAX_BYTES: usize = 997;

    const PREFIX: &'static str = "D ";

    /// Construct data response
    ///
    /// Returns error if response exceeds the limit set by assuan protocol (see [Data::MAX_BYTES])
    pub fn new(data: &str) -> Result<Self, TooLong> {
        let mut resp = Self::default();
        resp.append(data)?;
        Ok(resp)
    }

    /// Sets `Ok` response to be sent after the data
    pub fn with_custom_ok(mut self, ok: Ok) -> Self {
        self.ok = ok;
        self
    }

    /// Sets custom debug info for `OK` response returned after the data
    ///
    /// Returns error if response exceeds the limit set by assuan protocol (see [Ok::MAX_BYTES])
    pub fn with_debug_info(self, info: &str) -> Result<Self, TooLong> {
        Ok(self.with_custom_ok(Ok::with_debug_info(info)?))
    }

    /// Appends data to the response
    ///
    /// Returns error if response exceeds the limit set by assuan protocol (see [Data::MAX_BYTES])
    pub fn append(&mut self, data: &str) -> Result<(), TooLong> {
        self.data_resp.append(data)
    }

    /// Removes the last character from the response
    ///
    /// May not have great performance as each invocation requires UTF8 decoding of all
    /// response to find the last character position.
    ///
    /// ### Example
    /// ```rust
    /// use assuan_server::response::Data;
    ///
    /// let mut resp = Data::new("test")?;
    /// assert_eq!(resp.pop(), Some('t'));
    /// assert_eq!(resp.pop(), Some('s'));
    /// assert_eq!(resp.pop(), Some('e'));
    /// assert_eq!(resp.pop(), Some('t'));
    /// assert_eq!(resp.pop(), None);
    /// # Ok::<_, assuan_server::response::TooLong>(())
    /// ```
    pub fn pop(&mut self) -> Option<char> {
        if self.data_resp.size() == Self::PREFIX.len() {
            // Do not allow removing characters from the prefix
            return None;
        }
        self.data_resp.pop()
    }

    /// Indicated whether connection needs to be closed when response is sent
    pub fn close_connection(mut self, v: bool) -> Self {
        self.ok = self.ok.close_connection(v);
        self
    }
}

impl Default for Data {
    fn default() -> Self {
        Self {
            data_resp: ResponseLine::new()
                .chain(Self::PREFIX)
                .expect("prefix is much smaller than the limit"),
            ok: Default::default(),
        }
    }
}

impl zeroize::DefaultIsZeroes for Data {}

/// OK response
///
/// On a wire, OK response has format:
///
/// ```text
/// OK [escaped debug info]\n
/// ```
///
/// Response is UTF8 string. Certain characters in the string are percent-encoded (e.g. `\n` is transmitted as `%A0`).
/// Percent encoding is done automatically when response is written. Debug info is limited by [Ok::MAX_BYTES] size
/// in bytes after percent-encoding.
#[derive(Clone, Copy)]
pub struct Ok {
    resp: ResponseLine,
    close_conn: bool,
}

impl Ok {
    /// Max size of data response as specified in assuan spec
    ///
    /// Assuan spec sets the limit for max response size: 1000 bytes. 4 bytes of those are
    /// used for data prefix (`"OK "` of 3 bytes) and final `\n` byte indicating end of the
    /// response. So the response data may be up to 996 bytes long.
    pub const MAX_BYTES: usize = 996;

    /// Construct `OK` response with default message
    ///
    /// Default message is "success".
    pub fn new() -> Self {
        Self::with_debug_info("success").expect("debug info is not too long")
    }

    /// Constructs a new `OK` response with custom debug info
    ///
    /// Returns error if debug info exceeds limit set by the assuan spec (see [Ok::MAX_BYTES])
    pub fn with_debug_info(info: &str) -> Result<Self, TooLong> {
        Ok(Self {
            resp: ResponseLine::new().chain("OK ")?.chain(info)?,
            close_conn: false,
        })
    }

    /// Indicated whether connection needs to be closed when response is sent
    pub fn close_connection(mut self, v: bool) -> Self {
        self.close_conn = v;
        self
    }
}

impl Default for Ok {
    fn default() -> Self {
        Self::new()
    }
}

impl zeroize::DefaultIsZeroes for Ok {}

/// Response exceeds limit of [MAX_LINE_SIZE](crate::MAX_LINE_SIZE)
#[derive(Debug)]
pub struct TooLong;

pub(crate) use builder::ResponseLine;
mod builder {
    use super::TooLong;

    /// Response line constructor. Follows requirements enforced by assuan spec, including the percentage
    /// encoding, and size limit.
    ///
    /// We keep it in a separate private module to make sure that its private methods are not being
    /// used by rest of the parent module.
    #[derive(Clone, Copy)]
    pub struct ResponseLine {
        resp: [u8; Self::MAX_SIZE],
        size: usize,
    }

    impl ResponseLine {
        /// Max size of response line. Derived from the assuan specs that specify max size of the line,
        /// plus we reserve one byte for newline character.
        const MAX_SIZE: usize = crate::MAX_LINE_SIZE - 1;

        /// Initializes a new constructor
        pub fn new() -> Self {
            Self {
                resp: [0u8; Self::MAX_SIZE],
                size: 0,
            }
        }

        /// Size of response line in bytes
        pub fn size(&self) -> usize {
            self.size
        }

        /// Appends data to the response
        ///
        /// Similar to `append` but consumed `self` instead of taking a mutable reference
        pub fn chain(mut self, data: &str) -> Result<Self, TooLong> {
            self.append(data)?;
            Ok(self)
        }

        /// Appends data to the response
        ///
        /// Data must be a valid UTF-8 string no longer than 1000 bytes (including the final `\n` symbol that's
        /// put automatically). Returns error if the data exceeds the size limit.
        pub fn append(&mut self, mut data: &str) -> Result<(), TooLong> {
            if data.len() > self.resp.len() - self.size {
                return Err(TooLong);
            }

            loop {
                let mut iter = data.char_indices();
                let Some((pos, x)) = iter.find_map(|(i, x)| Some((i, optionally_escape(x)?)))
                else {
                    // There's nothing to be escaped, we can just copy the string
                    self.add_data(data)?;
                    return Ok(());
                };

                // A symbol that needs to be escaped is found at position `pos`.
                // The whole string up to this symbol can be copied without
                // modification
                self.add_data(&data[..pos])?;

                // Write escaped symbol
                self.add_data(x)?;

                // Continue parsing the string
                data = iter.as_str();
            }
        }

        fn add_data(&mut self, data: impl AsRef<[u8]>) -> Result<(), TooLong> {
            let data_len = data.as_ref().len();
            if data_len == 0 {
                return Ok(());
            }

            let out = self
                .resp
                .get_mut(self.size..self.size + data_len)
                .ok_or(TooLong)?;
            self.size += data_len;
            out.copy_from_slice(data.as_ref());
            Ok(())
        }

        /// Deletes the last symbol from the response and returns it
        ///
        /// Be aware that this method doesn't have great performance as `ResponseLine` stores
        /// the string as bytes, and removing last character requires UTF8 decoding to find
        /// the range of the last symbol which is not efficient.
        pub fn pop(&mut self) -> Option<char> {
            let s = std::str::from_utf8(&self.resp[..self.size])
                .expect("response is guaranteed to be a valid utf8 string");
            let (pos, x) = s.char_indices().next_back()?;
            self.size = pos;
            Some(x)
        }

        /// Writes response to the writer
        pub fn write(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
            out.write_all(&self.resp[..self.size])?;
            out.write_all(b"\n")
        }
    }

    impl Default for ResponseLine {
        fn default() -> Self {
            Self::new()
        }
    }

    impl zeroize::DefaultIsZeroes for ResponseLine {}

    /// Escapes char if it needs to be escaped, returns `None` otherwise
    fn optionally_escape(x: char) -> Option<&'static str> {
        match x {
            '%' => Some("%25"),
            '\r' => Some("%0D"),
            '\n' => Some("%0A"),
            '\\' => Some("%5C"),
            _ => None,
        }
    }
}
