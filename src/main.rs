use blog_builder::Config;
use std::env;
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::new(env::args()).unwrap_or_else(|err| {
        eprintln!("error: {err}");
        process::exit(1)
    });
    if let Err(e) = blog_builder::run(config) {
        eprintln!("error: {e}");
        process::exit(1)
    }
    Ok(())
}
