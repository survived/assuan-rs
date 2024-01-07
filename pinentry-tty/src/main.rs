fn main() -> std::io::Result<()> {
    let mut server = pinentry_tty::server();

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    server.serve_client(stdin, stdout)
}
