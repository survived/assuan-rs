use std::io;

/// Parses lines from the [`io::Read`]
///
/// Lines are restricted to be no more than 1000 bytes long, as specified in assuan specs
pub struct LineReader {
    bytes_read: usize,
    newline_found: Option<usize>,
    buffer: [u8; crate::MAX_LINE_SIZE],
}

impl LineReader {
    /// Constructs the parser
    pub const fn new() -> Self {
        Self {
            bytes_read: 0,
            newline_found: None,
            buffer: [0u8; crate::MAX_LINE_SIZE],
        }
    }

    /// Reads a line from the `reader`
    ///
    /// Returns the line without trailing newline character `\n`. If there's no data available, returns `None`.
    /// Returns error if `reader` returned error, or if invalid bytes received.
    pub fn read_line(
        &mut self,
        reader: &mut impl io::Read,
    ) -> Result<Option<&[u8]>, ReadLineError> {
        if let Some(newline_pos) = self.newline_found.take() {
            // We still store a line from previous `read_line` invocation. Gotta clear
            // that out
            self.bytes_read -= newline_pos + 1;
            self.buffer.copy_within(newline_pos + 1.., 0);
        }

        // There's some unprocessed bytes from previous `read_line` invocation.
        // Check if it has a newline.
        if self.bytes_read != 0 {
            if let Some(pos) = self.buffer[..self.bytes_read]
                .iter()
                .position(|c| *c == b'\n')
            {
                self.newline_found = Some(pos);
                return Ok(Some(&self.buffer[..pos]));
            }
        }

        // Read bytes until we find a newline character
        while self.bytes_read < crate::MAX_LINE_SIZE {
            let chunk_start = self.bytes_read;
            let chunk_size = reader
                .read(&mut self.buffer[chunk_start..])
                .map_err(ReadLineError::Read)?;
            self.bytes_read += chunk_size;

            match (chunk_start, chunk_size) {
                (0, 0) => return Ok(None),
                (_, 0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                _ => (),
            }
            if let Some(newline_pos) = self.buffer[chunk_start..chunk_start + chunk_size]
                .iter()
                .position(|c| *c == b'\n')
                .map(|p| p + chunk_start)
            {
                self.newline_found = Some(newline_pos);
                return Ok(Some(&self.buffer[..newline_pos]));
            }
        }

        Err(ReadLineError::LineTooLong)
    }
}

#[derive(Debug)]
pub enum ReadLineError {
    Read(io::Error),
    LineTooLong,
}

impl From<io::ErrorKind> for ReadLineError {
    fn from(kind: io::ErrorKind) -> Self {
        ReadLineError::Read(kind.into())
    }
}

#[cfg(test)]
mod test {
    use std::{io, iter};

    use super::LineReader;

    struct ReadChunks<I> {
        chunks: I,
    }

    impl<I> ReadChunks<I> {
        pub fn from_iter(chunks: impl IntoIterator<IntoIter = I>) -> Self {
            ReadChunks {
                chunks: chunks.into_iter(),
            }
        }
    }

    fn read_chunk_by_chunk<'a>(
        chunks: &'a [&'a [u8]],
    ) -> ReadChunks<impl Iterator<Item = &'a [u8]>> {
        ReadChunks::from_iter(chunks.iter().copied())
    }

    impl<'a, I> io::Read for ReadChunks<I>
    where
        I: Iterator<Item = &'a [u8]>,
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(chunk) = self.chunks.next() {
                assert!(buf.len() >= chunk.len(), "chunk len exceeds buf len");
                buf[..chunk.len()].copy_from_slice(chunk);
                Ok(chunk.len())
            } else {
                Ok(0)
            }
        }
    }

    #[test]
    fn reads_nothing() {
        let mut reader = LineReader::new();
        let mut read = ReadChunks::from_iter(iter::empty());

        let line = reader.read_line(&mut read).unwrap();
        assert_eq!(line, None);
    }

    #[test]
    fn reads_one_line() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"a line\n"]);

        let line = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line, b"a line");
    }

    #[test]
    fn reads_two_lines() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"line1\n", b"line2\n"]);

        let line1 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line1, b"line1");

        let line2 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line2, b"line2");
    }

    #[test]
    fn reads_two_lines_in_one_call() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"line1\nline2\n"]);

        let line1 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line1, b"line1");

        let line2 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line2, b"line2");
    }

    #[test]
    fn reads_one_line_in_pieces() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"a very", b" long ", b"line\n"]);

        let line = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line, b"a very long line");
    }

    #[test]
    fn reads_one_line_and_piece_of_second_in_one_call() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"a line\nand the", b" second one\n"]);

        let line1 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line1, b"a line");

        let line2 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line2, b"and the second one");
    }

    #[test]
    fn reads_line_and_terminates() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"a line\n"]);

        let line1 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line1, b"a line");

        let line2 = reader.read_line(&mut read).unwrap();
        assert_eq!(line2, None);
    }

    #[test]
    fn errors_on_unexpected_eof() {
        let mut reader = LineReader::new();
        let mut read = read_chunk_by_chunk(&[b"a line\nbut", b"the 2nd is not terminated"]);

        let line1 = reader.read_line(&mut read).unwrap().unwrap();
        assert_eq!(line1, b"a line");

        let err = reader.read_line(&mut read).unwrap_err();
        assert!(
            matches!(&err, super::ReadLineError::Read(err) if err.kind() == io::ErrorKind::UnexpectedEof),
            "{err:?} is not what we expected to see"
        )
    }

    #[test]
    fn errors_on_very_large_line() {
        let mut reader = LineReader::new();
        let hundred_bytes = [1u8; 100];
        let chunks_of_1000_bytes = [hundred_bytes.as_slice(); 10];
        let mut read = read_chunk_by_chunk(chunks_of_1000_bytes.as_slice());

        let err = reader.read_line(&mut read).unwrap_err();
        assert!(
            matches!(&err, super::ReadLineError::LineTooLong),
            "{err:?} is not what we expected to see"
        );
    }
}
