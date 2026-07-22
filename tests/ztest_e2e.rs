//! End-to-end coverage of the `@test` runner: `eval_test_inputs` (discovery +
//! input evaluation) followed by `circ::test_runner::run_test` (compile ->
//! assert check -> setup -> prove -> verify through Groth16/BLS12-381). This is the
//! path the `ztest` example exercises; `tests/zok_test_inputs.rs` stops at
//! input evaluation, so without this file a regression in flattening, the IR
//! optimization pipeline, R1CS lowering, or the outcome semantics could merge
//! with every registered test green.
//!
//! Assertions are on the structured `Outcome`, not on printed text.

#![cfg(all(feature = "smt", feature = "zokc", feature = "bellman"))]

use bls12_381::Bls12;
use circ::front::zsharpcurly::{TestCase, ZSharpCurlyFE};
use circ::front::Mode;
use circ::target::r1cs::bellman::Bellman;
use circ::test_runner::{run_test, Outcome};
use std::path::{Path, PathBuf};
use std::sync::Once;

static INIT: Once = Once::new();

/// A temp directory (with its contents) removed on drop, even on panic.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write `files` into a unique temp directory (so sibling imports resolve by
/// bare name) and return the guard plus the path to the first file (the entry).
fn write_files(test_name: &str, files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    INIT.call_once(circ::cfg::set_default);
    let dir = std::env::temp_dir().join(format!("ztest_e2e_{}_{}", std::process::id(), test_name));
    std::fs::create_dir_all(&dir).unwrap();
    let guard = TempDir(dir);
    for (name, src) in files {
        std::fs::write(guard.0.join(name), src).unwrap();
    }
    let entry = guard.0.join(files[0].0);
    (guard, entry)
}

/// Single-file convenience wrapper over [`write_files`].
fn write_file(test_name: &str, src: &str) -> (TempDir, PathBuf) {
    write_files(test_name, &[("main.zok", src)])
}

/// Discover the `@test` functions in `path`.
fn discover(path: &Path) -> Vec<TestCase> {
    ZSharpCurlyFE::eval_test_inputs(path.to_path_buf(), Mode::Proof).unwrap()
}

/// Discover and run every test in a single-test file; return its outcome.
fn run_only(test_name: &str, src: &str) -> Outcome {
    let (_guard, path) = write_file(test_name, src);
    let tests = discover(&path);
    assert_eq!(tests.len(), 1, "expected exactly one @test function");
    run_test::<Bellman<Bls12>>(&tests[0])
}

#[test]
fn nested_array_passes_full_pipeline() {
    let outcome = run_only(
        "nested_array",
        r#"@test A = [[1, 2], [3, 4]];
def test_mat(private field[2][2] A) {
    assert(A[0][0] == 1);
    assert(A[1][1] == 4);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn array_initializer_passes() {
    // Sparse [0; 4]: the flatten path must still emit all four leaves.
    let outcome = run_only(
        "array_fill",
        r#"@test xs = [0; 4];
def test_fill(private field[4] xs) {
    assert(xs[0] + xs[1] + xs[2] + xs[3] == 0);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn public_and_private_array_leaves_pass() {
    // Matrix multiply with private inputs and a PUBLIC expected array C: C's
    // leaves must reach the verifier map. Product independently confirmed via
    // zcxi against mm.zok: [[1,2],[3,4]] x [[5,6],[7,8]] = [[19,22],[43,50]].
    let outcome = run_only(
        "public_private",
        r#"@test A = [[1, 2], [3, 4]], B = [[5, 6], [7, 8]], C = [[19, 22], [43, 50]];
def test_mm(private field[2][2] A, private field[2][2] B, public field[2][2] C) {
    field[2][2] P = [[0; 2]; 2];
    for field i in 0..2 {
        for field j in 0..2 {
            for field k in 0..2 {
                P[i][j] = P[i][j] + A[i][k] * B[k][j];
            }
        }
    }
    for field i in 0..2 {
        for field j in 0..2 {
            assert(P[i][j] == C[i][j]);
        }
    }
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn same_file_named_const_array_passes() {
    let outcome = run_only(
        "same_file_const",
        r#"const field[2][2] IMG = [[1, 0], [0, 1]];

@test A = IMG;
def test_img(private field[2][2] A) {
    assert(A[0][0] == 1);
    assert(A[1][1] == 1);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn imported_const_array_passes() {
    let (_guard, path) = write_files(
        "imported_const",
        &[
            (
                "main.zok",
                r#"from "helper" import IMG;

@test A = IMG;
def test_img(private field[2][2] A) {
    assert(A[0][0] == 1);
    assert(A[1][1] == 1);
}
"#,
            ),
            ("helper.zok", "const field[2][2] IMG = [[1, 0], [0, 1]];\n"),
        ],
    );
    let tests = discover(&path);
    assert_eq!(tests.len(), 1);
    let outcome = run_test::<Bellman<Bls12>>(&tests[0]);
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn non_square_array_passes() {
    // A non-square field[2][3]: the shape itself (2 rows of 3, not 3 of 2)
    // would break if flattening reversed dimension order — a square matrix
    // could hide that. Distinct values pin the row/column mapping.
    let outcome = run_only(
        "non_square",
        r#"@test M = [[1, 2, 3], [4, 5, 6]];
def test_ns(private field[2][3] M) {
    assert(M[0][0] == 1);
    assert(M[0][2] == 3);
    assert(M[1][0] == 4);
    assert(M[1][2] == 6);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn three_dimensional_array_passes() {
    let outcome = run_only(
        "three_dim",
        r#"@test T = [[[1, 2], [3, 4]], [[5, 6], [7, 8]]];
def test_3d(private field[2][2][2] T) {
    assert(T[0][0][0] == 1);
    assert(T[1][0][1] == 6);
    assert(T[1][1][1] == 8);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn uint_array_passes() {
    let outcome = run_only(
        "uint_array",
        r#"@test ws = [1, 2, 3, 4];
def test_u32s(private u32[4] ws) {
    assert(ws[0] + ws[1] == ws[2]);
    assert(ws[3] == 4u32);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn wrong_nested_array_value_is_assertion_failure() {
    // A false assertion is a SEMANTIC rejection (AssertionFailed), never a
    // backend error. This is the signal a future `expect = reject` keys off.
    let outcome = run_only(
        "wrong_nested",
        r#"@test A = [[1, 2], [3, 4]];
def test_mat(private field[2][2] A) {
    assert(A[1][1] == 99);
}
"#,
    );
    assert!(
        matches!(outcome, Outcome::AssertionFailed(_)),
        "got {:?}",
        outcome
    );
}

#[test]
fn linear_all_private_false_assertion_is_assertion_failure() {
    // The vacuous-pass class: a purely linear assert on a private scalar. The
    // proof pipeline alone would pass this vacuously (the private var gets
    // optimized away); the ground-truth check must still classify it as
    // AssertionFailed.
    let outcome = run_only(
        "linear_private",
        r#"@test x = 4;
def test_linear(private field x) {
    assert(x == 99);
}
"#,
    );
    assert!(
        matches!(outcome, Outcome::AssertionFailed(_)),
        "got {:?}",
        outcome
    );
}
