//! Unit-test runner for ZoKratesCurly programs.
//! Reads a .zok file, finds every function marked with a @test annotation,
//! evaluates its annotation inputs to concrete values, then runs each test
//! through the full in-memory proof pipeline:
//!   compile (test fn as entry point) -> assert check -> setup -> prove -> verify
//! and prints ok / FAILED per test. A test passes when its assertions
//! evaluate to true on the given inputs (checked by direct IR evaluation —
//! the proof pipeline alone can pass vacuously when optimization eliminates
//! a private variable) AND a proof can be produced and verifies (backend:
//! Groth16 over BLS12-381, hardcoded for the MVP).
//!
//! The per-test execution logic lives in [`circ::test_runner`]; this file is
//! the CLI wrapper.
use bls12_381::Bls12;
use circ::cfg::{clap, CircOpt};
use circ::front::zsharpcurly::ZSharpCurlyFE;
use circ::front::Mode;
use circ::ir::term::Value;
use circ::target::r1cs::bellman::Bellman;
use circ::test_runner::{catch, run_test, Outcome};
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "ztest",
    about = "Run @test functions in a ZoKratesCurly program"
)]
struct Options {
    /// Input file
    #[arg(name = "PATH")]
    path: PathBuf,

    #[command(flatten)]
    circ: CircOpt,
}

/// Render a concrete Value as a plain number/bool, without the field
/// modulus that Value's own Display appends (e.g. `9` instead of `#f9m524...`).
fn pretty_value(v: &Value) -> String {
    match v {
        Value::Field(f) => f.i().to_string(),
        Value::BitVector(b) => b.uint().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => format!(
            "[{}]",
            a.values()
                .iter()
                .map(pretty_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // Anything else (tuples, ...): fall back to the default form.
        other => other.to_string(),
    }
}

fn main() {
    env_logger::Builder::from_default_env()
        .format_level(false)
        .format_timestamp(None)
        .init();

    let options = Options::parse();
    circ::cfg::set(&options.circ);

    // The frontend panics on a missing file with an unhelpful unwrap
    // message; check up front so the user gets a plain answer.
    if !options.path.exists() {
        eprintln!("Error: file not found: {}", options.path.display());
        std::process::exit(1);
    }

    // Find every @test function and evaluate its annotation inputs to
    // concrete values. Parse errors surface as panics, evaluation errors
    // through the Result.
    let tests = match catch(|| ZSharpCurlyFE::eval_test_inputs(options.path.clone(), Mode::Proof)) {
        Ok(Ok(tests)) => tests,
        Ok(Err(e)) | Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!(
        "running {} test{} from {}",
        tests.len(),
        if tests.len() == 1 { "" } else { "s" },
        options.path.display()
    );

    let (mut passed, mut failed, mut errored) = (0, 0, 0);
    for t in &tests {
        // Show each input as `name = <source> = <value>`, collapsing to
        // `name = <value>` when the source is already just the value.
        let inputs: Vec<String> = t
            .inputs()
            .iter()
            .map(|input| {
                let value = pretty_value(input.value());
                if input.source() == value {
                    format!("{} = {}", input.name(), value)
                } else {
                    format!("{} = {} = {}", input.name(), input.source(), value)
                }
            })
            .collect();
        print!("test {} ({}) ... ", t.name(), inputs.join(", "));
        // Flush so the prover's own stderr diagnostics (printed when a
        // constraint fails) appear under this test's line, not before it.
        use std::io::Write;
        let _ = std::io::stdout().flush();

        // Groth16 over BLS12-381 is hardcoded for the MVP; run_test is
        // generic over the proof system, so this is the one place the backend
        // is chosen.
        let indent = |msg: String| {
            // The prover's message spans several lines; indent them all.
            for line in msg.lines() {
                println!("    {}", line);
            }
        };
        match run_test::<Bellman<Bls12>>(t) {
            Outcome::Pass => {
                passed += 1;
                println!("ok");
            }
            Outcome::AssertionFailed(msg) => {
                failed += 1;
                println!("FAILED");
                indent(msg);
            }
            Outcome::CompileError(msg) | Outcome::BackendError(msg) => {
                errored += 1;
                println!("error");
                indent(msg);
            }
        }
    }

    println!(
        "\ntest result: {}. {} passed; {} failed; {} errored",
        if failed == 0 && errored == 0 {
            "ok"
        } else {
            "FAILED"
        },
        passed,
        failed,
        errored
    );
    if failed > 0 || errored > 0 {
        std::process::exit(1);
    }
}
