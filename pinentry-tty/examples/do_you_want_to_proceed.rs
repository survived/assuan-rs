#[derive(PartialEq, Eq)]
enum Choice {
    Yes,
    No,
}

fn main() -> std::io::Result<()> {
    let choice = pinentry_tty::dialog(
        "Do you want to proceed?",
        &[("Yes", Choice::Yes), ("No", Choice::No)],
    )?;

    if choice == Some(&Choice::Yes) {
        // Do something
    }

    Ok(())
}
