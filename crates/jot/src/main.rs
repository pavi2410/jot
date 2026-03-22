mod cli;
mod commands;
mod init_templates;
mod utils;

fn main() {
    if let Err(error) = commands::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
