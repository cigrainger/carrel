//! carrel-app: desktop application shell.

#![deny(unsafe_code)]

fn main() {
    if let Err(error) = carrel_app::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
