use std::io;

pub struct Capture<S, O> {
    source: S,
    output: O,
    prepend: &'static [u8],
    buffer: Vec<u8>,
}

impl<S, O: io::Write> Capture<S, O> {
    fn more_data(&mut self, data: &[u8]) -> io::Result<()> {
        self.buffer.extend_from_slice(data);

        while let Some(pos) = self.buffer.iter().position(|x| *x == b'\n') {
            self.output.write_all(self.prepend)?;
            self.output.write_all(&self.buffer[..pos])?;
            self.output.write_all(b"\\n\n")?;
            self.output.flush()?;

            if pos + 1 < self.buffer.len() {
                self.buffer.copy_within(pos + 1.., 0);
                self.buffer.truncate(self.buffer.len() - (pos + 1));
            } else {
                self.buffer.clear();
            }
        }

        Ok(())
    }
}

impl<I, O> io::Read for Capture<I, O>
where
    I: io::Read,
    O: io::Write,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes = self.source.read(buf)?;
        self.more_data(&buf[..bytes])?;
        Ok(bytes)
    }
}

fn main() {
    let mut args = std::env::args().peekable();
    let _prog = args.next().unwrap();

    let (output, executable) = match (args.next(), args.next()) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            eprintln!("Usage: ./assuan-hijack OUTPUT_PATH EXECUTABLE_PATH [--] [args..]");
            std::process::exit(1);
        }
    };

    if args.peek().map(String::as_str) == Some("--") {
        let _ = args.next();
    }

    let output = || {
        std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open(&output)
            .expect("couldn't open output file")
    };
    let out_reqs = output();
    let out_resps = output();

    let mut child = std::process::Command::new(executable)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("start executable");

    let mut child_stdin = child.stdin.take().expect("couldn't capture stdin");
    let child_stdout = child.stdout.take().expect("couldn't capture stdout");

    let handle_requests = std::thread::spawn(move || {
        let stdin = std::io::stdin().lock();
        let mut capture_client_requests = Capture {
            source: stdin,
            output: out_reqs,
            prepend: b"C: ",
            buffer: Vec::with_capacity(1000),
        };
        std::io::copy(&mut capture_client_requests, &mut child_stdin).expect("copying failed");
    });
    let handle_responses = std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        let mut capture_server_responses = Capture {
            source: child_stdout,
            output: out_resps,
            prepend: b"S: ",
            buffer: Vec::with_capacity(1000),
        };
        std::io::copy(&mut capture_server_responses, &mut stdout).expect("copying failed")
    });

    handle_requests.join().expect("handle requests error");
    handle_responses.join().expect("handle responses error");
}
