/// Simple ZoKratesCurly syntax and type checker
/// Equivalent to `zokrates check` but for ZoKratesCurly (.zok files with curly braces)

use circ::cfg::{clap, CircOpt};
use clap::Parser;
use circ::front::zsharpcurly::{Inputs, ZSharpCurlyFE};
use circ::front::{FrontEnd, Mode};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "zcheck_curly", about = "Check ZoKratesCurly programs for syntax and type errors")]
struct Options {
    /// Input file
    #[arg(name = "PATH")]
    path: PathBuf,

    #[command(flatten)]
    circ: CircOpt,
}

fn main() {
    env_logger::Builder::from_default_env()
        .format_level(false)
        .format_timestamp(None)
        .init();

    let options = Options::parse();
    circ::cfg::set(&options.circ);

    let inputs = Inputs {
        file: options.path,
        mode: Mode::Proof,
    };

    // Try to generate the circuit - this will parse and type check
    match std::panic::catch_unwind(|| {
        ZSharpCurlyFE::gen(inputs)
    }) {
        Ok(_) => {
            println!("✓ Program is valid");
            std::process::exit(0);
        }
        Err(e) => {
            // Parsing errors are panics, extract the message if possible
            if let Some(msg) = e.downcast_ref::<String>() {
                eprintln!("Error: {}", msg);
            } else if let Some(msg) = e.downcast_ref::<&str>() {
                eprintln!("Error: {}", msg);
            } else {
                eprintln!("Error: Unknown parsing error");
            }
            std::process::exit(1);
        }
    }
}
