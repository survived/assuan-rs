use assuan::response::{Data, Response, TooLong};

struct Greeter {
    my_name: &'static str,
}

impl Greeter {
    fn greet(&mut self, client_name: Option<&str>) -> Result<Response, TooLong> {
        let mut resp = Data::new("Hello, ")?;
        resp.append(client_name.unwrap_or("anon"))?;
        resp.append("! My name's ")?;
        resp.append(self.my_name)?;
        Ok(resp.into())
    }
}

fn main() -> std::io::Result<()> {
    let greeter = Greeter { my_name: "Alice" };

    assuan::AssuanServer::new(greeter)
        .add_command("GREET", Greeter::greet)
        .serve_client(std::io::stdin(), std::io::stdout())
}
