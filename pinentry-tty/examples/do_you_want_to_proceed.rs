#[derive(PartialEq, Eq)]
enum Choice {
    Yes,
    No,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tty = pinentry_tty::Termion::new_stdio()?;

    let choice = pinentry_tty::dialog(
        &mut tty,
        "Do you want to proceed?",
        &[("Yes", Choice::Yes), ("No", Choice::No)],
    )?;

    if choice == Some(&Choice::Yes) {
        // Do something
    }

    Ok(())
}
