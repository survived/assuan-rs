pub enum Response {
    Data(Data),
    Ok(Ok),
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
    pub fn ok() -> Self {
        Self::Ok(Ok::new())
    }

    pub fn ok_with_debug_info(info: &str) -> Result<Self, TooLong> {
        Ok::with_debug_info(info).map(Self::Ok)
    }

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
        }
    }

    /// Indicates whether a connection needs to be closed when response is sent
    pub fn connection_needs_be_closed(&self) -> bool {
        match self {
            Self::Ok(r) => r.close_conn,
            Self::Data(r) => r.ok.close_conn,
        }
    }
}

pub struct Data {
    data_resp: ResponseLine,
    ok: Ok,
}

impl Data {
    pub fn new(data: &str) -> Result<Self, TooLong> {
        Ok(Self {
            data_resp: ResponseLine::new().append("D ")?.append(data)?,
            ok: Ok::new(),
        })
    }

    pub fn with_custom_ok(mut self, ok: Ok) -> Self {
        self.ok = ok;
        self
    }

    pub fn with_debug_info(self, info: &str) -> Result<Self, TooLong> {
        Ok(self.with_custom_ok(Ok::with_debug_info(info)?))
    }

    /// Indicated whether connection needs to be closed when response is sent
    pub fn close_connection(mut self, v: bool) -> Self {
        self.ok = self.ok.close_connection(v);
        self
    }
}

pub struct Ok {
    resp: ResponseLine,
    close_conn: bool,
}

impl Ok {
    pub fn new() -> Self {
        Self::with_debug_info("success").expect("debug info is not too long")
    }

    pub fn with_debug_info(info: &str) -> Result<Self, TooLong> {
        Ok(Self {
            resp: ResponseLine::new().append("OK ")?.append(info)?,
            close_conn: false,
        })
    }

    /// Indicated whether connection needs to be closed when response is sent
    pub fn close_connection(mut self, v: bool) -> Self {
        self.close_conn = v;
        self
    }
}

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

        /// Appends data to the response
        ///
        /// Data must be a valid UTF-8 string no longer than 1000 bytes (including the final `\n` symbol that's
        /// put automatically). Returns error if the data exceeds the size limit.
        pub fn append(mut self, mut data: &str) -> Result<Self, TooLong> {
            loop {
                let mut iter = data.char_indices();
                let Some((pos, x)) = iter.find_map(|(i, x)| Some((i, optionally_escape(x)?)))
                else {
                    // There's nothing to be escaped, we can just copy the string
                    self.add_data(data)?;
                    return Ok(self);
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

        pub fn write(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
            out.write_all(&self.resp[..self.size])?;
            out.write_all(b"\n")
        }
    }

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
