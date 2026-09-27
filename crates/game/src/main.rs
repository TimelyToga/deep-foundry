//! Deep Foundry: the game program.
//!
//! - With no options: open a window, run the simulation on its own thread, and draw it.
//! - With `--screenshot`: render one image with no window and quit.
//!
//! See README.md in this folder for the controls.

mod app;
mod args;
mod controls;
mod debug_panel;
mod demo;
mod saves;
mod screenshot;
mod sim_thread;
mod smoke;
mod ui;

use anyhow::Result;
use foundry_content::Content;
use std::sync::Arc;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,deep_foundry=info,foundry_render=info"),
    )
    .init();
    let args = match args::parse(std::env::args().skip(1)) {
        Ok(args::Parsed::Run(a)) => a,
        Ok(args::Parsed::Help) => {
            print!("{}", args::USAGE);
            return Ok(());
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    };
    let content = Arc::new(Content::load_default()?);
    match &args.screenshot {
        Some(out) => screenshot::run(&args, out, content),
        None => app::run(args, content),
    }
}
