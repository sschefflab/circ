//! Executes one `@test` function end-to-end through the proof pipeline.
//!
//! This is the reusable core behind the `ztest` example: compile the test
//! function as its own entry point, check its assertions against the resolved
//! inputs, then run the full `setup -> prove -> verify` cycle in memory.
//!
//! It sits *above* the frontend on purpose: the ZoKrates frontend produces
//! validated test metadata ([`TestCase`]) and CirC IR; this module consumes
//! that read-only API and orchestrates compilation ([`crate::compile`]) and
//! the proof backend ([`crate::target::r1cs`]). The frontend does not depend
//! back on it. It is generic over the proof system ([`ProofSystem`]) so the
//! backend is chosen by the caller (the example instantiates Groth16 over
//! BLS12-381). That generic covers the [`ProofSystem`] implementors — Bellman
//! (Groth16) and Mirage — and is where their selection will plug in; Spartan
//! uses a separate `SpartanProofSystem` interface (and a different field), so
//! supporting it will take an adapter, not just a type argument. Presentation
//! — printing, exit codes, input formatting — stays in the caller, not here.

use crate::cfg::cfg;
use crate::compile::{opt_for_proof, to_proof_data};
use crate::front::zsharpcurly::{Inputs, TestCase, ZSharpCurlyFE};
use crate::front::{FrontEnd, Mode};
use crate::ir::term::{eval, Value};
use crate::target::r1cs::proof::ProofSystem;
use fxhash::FxHashMap;
use std::cell::Cell;
use std::sync::Once;

/// The outcome of running one test.
///
/// Semantic outcomes (the test's assertions were accepted or rejected) are
/// kept distinct from infrastructure outcomes (the test could not be run, or a
/// backend step failed). A future `expect = accept|reject` setting keys off
/// [`Outcome::Pass`] / [`Outcome::AssertionFailed`] only — it must never treat
/// a [`Outcome::CompileError`] or [`Outcome::BackendError`] as a rejection.
#[derive(Debug)]
pub enum Outcome {
    /// Assertions held for the given inputs and the proof verified.
    Pass,
    /// An assertion does not hold for the given inputs. This is the semantic
    /// "the test rejected these inputs" signal.
    AssertionFailed(String),
    /// The test's shape is not runnable (e.g. it declares a return type, which
    /// carries no expected output to check).
    Unsupported(String),
    /// The frontend, IR optimization, or R1CS lowering failed — the test could
    /// not be turned into a circuit.
    CompileError(String),
    /// The backend failed to set up, prove, or verify a test whose assertions
    /// already held. This is an infrastructure failure, NOT a rejection.
    BackendError(String),
}

thread_local! {
    /// Set while a [catch] call is running on this thread, so the shared panic
    /// hook stays quiet for *this thread's* panics only.
    static SUPPRESS_PANIC: Cell<bool> = const { Cell::new(false) };
}

static HOOK_INIT: Once = Once::new();

/// Install, once per process, a panic hook that defers to the previous hook
/// except while the current thread is inside [catch]. Swapping the global hook
/// per call (take/quiet/restore) races under concurrency — two overlapping
/// calls can restore in the wrong order and leave the quiet hook installed for
/// good, or suppress an unrelated thread's panic. A thread-local flag consulted
/// by one installed-once hook avoids both: suppression is per-thread and the
/// global hook is never swapped after startup.
fn install_quiet_hook() {
    HOOK_INIT.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !SUPPRESS_PANIC.with(|s| s.get()) {
                prev(info);
            }
        }));
    });
}

/// Run `f`, converting a panic on the current thread into `Err(message)`.
///
/// The frontend and the prover report failures by panicking, so callers must
/// unwind them into a value. Panic-hook noise is suppressed for the duration —
/// but only for this thread (see [install_quiet_hook]), so concurrent code and
/// later panics keep the normal hook. Cannot contain `process::exit` paths
/// (some frontend semantic errors exit rather than unwind).
///
/// Exposed for callers that must also survive panics from the discovery phase
/// (e.g. [`ZSharpCurlyFE::eval_test_inputs`] panics on a parse/load failure).
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    install_quiet_hook();
    let was = SUPPRESS_PANIC.with(|s| s.replace(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    SUPPRESS_PANIC.with(|s| s.set(was));
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

/// Run one `@test` function through compile -> assert check -> setup -> prove
/// -> verify, using the proof system `PS`.
///
/// The test is recompiled from its own source file ([`TestCase::file`]), so the
/// case stays bound to the file it was discovered in — the caller cannot pair a
/// case with an unrelated path. The config is read from the process-global
/// [`cfg`]; the caller must have set it (via `circ::cfg::set` or `set_default`)
/// and it fixes the field/backend for the process.
pub fn run_test<PS: ProofSystem>(test: &TestCase) -> Outcome {
    // Assert-style tests only: a return-typed test computes a value, but the
    // annotation gives no expected output to check it against.
    if test.has_return() {
        return Outcome::Unsupported(
            "test functions with a return type are not supported (yet); \
             drop the return type and use assert(...)"
                .to_string(),
        );
    }

    // Compile the test function as its own entry point, from the file it was
    // discovered in. A failure here means the test never became a circuit.
    let name = test.name().to_string();
    let file = test.file().to_path_buf();
    let comps = match catch(move || {
        ZSharpCurlyFE::gen(Inputs {
            file,
            entry: name,
            mode: Mode::Proof,
        })
    }) {
        Ok(c) => c,
        Err(e) => return Outcome::CompileError(e),
    };

    // Build the prover and verifier input maps in a single pass over the
    // inputs — one flatten per input, not one per map. The prover knows every
    // input; the verifier sees only the public ones. Arrays flatten to one
    // entry per leaf ("A.0.0", ...), the names the compiled circuit declares
    // its input variables under; scalars are a single bare-named entry.
    let mut prover_map: FxHashMap<String, Value> = FxHashMap::default();
    let mut verifier_map: FxHashMap<String, Value> = FxHashMap::default();
    for input in test.inputs() {
        let entries = input.flat_entries();
        if input.public() {
            verifier_map.extend(entries.iter().cloned());
        }
        prover_map.extend(entries);
    }

    // Ground truth: evaluate every assertion directly against the prover's
    // inputs on the un-optimized IR. This is the authoritative pass/fail
    // verdict. The proof pipeline alone is not sufficient — it proves "a
    // satisfying witness exists", and when an optimization eliminates a private
    // variable (e.g. reduce_linearities substituting away a purely linear
    // assert like `x == 99`), the input-map value is never enforced and an
    // untrue test would pass vacuously.
    let held = catch(|| {
        comps
            .get(test.name())
            .outputs
            .iter()
            .all(|a| matches!(eval(a, &prover_map), Value::Bool(true)))
    });
    match held {
        Ok(true) => {}
        Ok(false) => {
            return Outcome::AssertionFailed(
                "an assertion does not hold for the given inputs".to_string(),
            )
        }
        // A panic while evaluating the compiled IR is an internal/compile-side
        // failure, not an assertion rejection.
        Err(e) => return Outcome::CompileError(e),
    }

    // Lower to prover/verifier data: the canonical proof-mode IR optimization
    // pipeline (shared with the CLI driver via opt_for_proof — required so
    // array/tuple terms are scalarized before R1CS lowering) followed by
    // R1CS lowering.
    let name = test.name().to_string();
    let lowered = catch(move || {
        let comps = opt_for_proof(comps, cfg());
        let cs = comps.get(&name);
        let (p_data, v_data, _stats) = to_proof_data(cs, cfg());
        (p_data, v_data)
    });
    let (p_data, v_data) = match lowered {
        Ok(d) => d,
        Err(e) => return Outcome::CompileError(e),
    };

    // Backend setup, then prove + verify. The assertions already held under
    // direct evaluation, so any failure from here on is a backend/infrastructure
    // failure — reported as BackendError, never as an assertion rejection.
    let setup = catch(move || PS::setup(p_data, v_data));
    let (pk, vk) = match setup {
        Ok(keys) => keys,
        Err(e) => return Outcome::BackendError(e),
    };
    match catch(|| {
        let pf = PS::prove(&pk, &prover_map);
        PS::verify(&vk, &verifier_map, &pf)
    }) {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::BackendError(
            "proof did not verify though assertions held for the given inputs".to_string(),
        ),
        Err(e) => Outcome::BackendError(e),
    }
}
