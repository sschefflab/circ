/// Unit-test runner for ZoKratesCurly programs.
/// Reads a .zok file, finds every function marked with a @test annotation,
/// evaluates its annotation inputs to concrete values, then runs each test
/// through the full in-memory proof pipeline:
///   compile (test fn as entry point) -> setup -> prove -> verify
/// and prints ok / FAILED per test. A test passes when its assertions hold,
/// i.e. when a proof can be produced and verifies (backend: Groth16 over
/// BLS12-381, hardcoded for the MVP).
use bls12_381::Bls12;
use circ::cfg::{clap, CircOpt};
use circ::front::zsharpcurly::{Inputs, TestCase, ZSharpCurlyFE};
use circ::front::{FrontEnd, Mode};
use circ::ir::term::Value;
use circ::target::r1cs::bellman::Bellman;
use circ::target::r1cs::proof::ProofSystem;
use clap::Parser;
use fxhash::FxHashMap;
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

/// The outcome of running one test.
enum Outcome {
    /// The proof verified: all assertions hold.
    Pass,
    /// Proving or verifying failed: an assertion does not hold.
    Fail(String),
    /// The test could not be run at all (e.g. unsupported shape).
    Error(String),
}

/// Render a concrete Value as a plain number/bool, without the field
/// modulus that Value's own Display appends (e.g. `9` instead of `#f9m524...`).
fn pretty_value(v: &Value) -> String {
    match v {
        Value::Field(f) => f.i().to_string(),
        Value::BitVector(b) => b.uint().to_string(),
        Value::Bool(b) => b.to_string(),
        // Anything else (arrays, tuples, ...): fall back to the default form.
        other => other.to_string(),
    }
}

/// Run `f`, catching panics and silencing the default panic hook only for
/// the duration of the call (the frontend and the prover report failures by
/// panicking). Returns the panic message on unwind.
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(prev_hook);
    result.map_err(|e| {
        if let Some(msg) = e.downcast_ref::<String>() {
            msg.clone()
        } else if let Some(msg) = e.downcast_ref::<&str>() {
            (*msg).to_string()
        } else {
            "unknown panic".to_string()
        }
    })
}

/// Run one test through compile -> setup -> prove -> verify.
fn run_test(path: &std::path::Path, test: &TestCase) -> Outcome {
    // MVP supports assert-style tests only: a return-typed test computes a
    // value, but the annotation gives no expected output to check it against.
    if test.has_return {
        return Outcome::Error(
            "test functions with a return type are not supported (yet); \
             drop the return type and use assert(...)"
                .to_string(),
        );
    }

    // Compile the test function as its own entry point and lower it to
    // prover/verifier data. Compilation failures are errors, not test
    // failures: the test never ran.
    let name = test.name.clone();
    let file = path.to_path_buf();
    let setup = catch(move || {
        let comps = ZSharpCurlyFE::gen(Inputs {
            file,
            entry: name.clone(),
            mode: Mode::Proof,
        });
        let cs = comps.get(&name);
        let (p_data, v_data, _stats) = circ::compile::to_proof_data(cs, circ::cfg::cfg());
        Bellman::<Bls12>::setup(p_data, v_data)
    });
    let (pk, vk) = match setup {
        Ok(keys) => keys,
        Err(e) => return Outcome::Error(e),
    };

    // The prover knows every input; the verifier sees only the public ones.
    let prover_map: FxHashMap<String, Value> = test
        .inputs
        .iter()
        .map(|i| (i.name.clone(), i.value.clone()))
        .collect();
    let verifier_map: FxHashMap<String, Value> = test
        .inputs
        .iter()
        .filter(|i| i.public)
        .map(|i| (i.name.clone(), i.value.clone()))
        .collect();

    // Prove and verify. An assertion that does not hold makes the witness
    // unsatisfiable, which the prover reports by panicking.
    match catch(|| {
        let pf = Bellman::<Bls12>::prove(&pk, &prover_map);
        Bellman::<Bls12>::verify(&vk, &verifier_map, &pf)
    }) {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Fail("proof did not verify".to_string()),
        Err(e) => Outcome::Fail(e),
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
            .inputs
            .iter()
            .map(|input| {
                let value = pretty_value(&input.value);
                if input.source == value {
                    format!("{} = {}", input.name, value)
                } else {
                    format!("{} = {} = {}", input.name, input.source, value)
                }
            })
            .collect();
        print!("test {} ({}) ... ", t.name, inputs.join(", "));
        // Flush so the prover's own stderr diagnostics (printed when a
        // constraint fails) appear under this test's line, not before it.
        use std::io::Write;
        let _ = std::io::stdout().flush();

        match run_test(&options.path, t) {
            Outcome::Pass => {
                passed += 1;
                println!("ok");
            }
            Outcome::Fail(msg) => {
                failed += 1;
                println!("FAILED");
                // The prover's message spans several lines; indent them all.
                for line in msg.lines() {
                    println!("    {}", line);
                }
            }
            Outcome::Error(msg) => {
                errored += 1;
                println!("error");
                for line in msg.lines() {
                    println!("    {}", line);
                }
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
