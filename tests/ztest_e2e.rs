//! End-to-end tests for the `@test` runner using Groth16 and Mirage over BLS12-381.
//!
//! `zok_test_inputs.rs` stops after discovery and input evaluation. These tests
//! continue through input flattening, compilation, assertion checking, IR
//! optimization, R1CS lowering, setup, proving, and verification. They check
//! the structured [`Outcome`] rather than CLI output.

#![cfg(all(feature = "smt", feature = "zokc", feature = "bellman"))]

use bls12_381::Bls12;
use circ::front::zsharpcurly::{TestBackend, TestCase, ZSharpCurlyFE};
use circ::front::Mode;
use circ::target::r1cs::{bellman::Bellman, mirage::Mirage};
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

fn run_selected(test: &TestCase) -> Outcome {
    match test.settings().backend() {
        TestBackend::Groth16 => run_test::<Bellman<Bls12>>(test),
        TestBackend::Mirage => run_test::<Mirage<Bls12>>(test),
    }
}

/// Discover and run every test in a single-test file; return its outcome.
fn run_only(test_name: &str, src: &str) -> Outcome {
    let (_guard, path) = write_file(test_name, src);
    let tests = discover(&path);
    assert_eq!(tests.len(), 1, "expected exactly one @test function");
    run_selected(&tests[0])
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
fn mixed_backends_pass_in_one_file() {
    let (_guard, path) = write_file(
        "mixed_backends",
        r#"@test(backend = groth16) x = 3;
def test_groth16(private field x) {
    assert(x * x == 9);
}

@test(backend = mirage) x = 4;
def test_mirage(private field x) {
    assert(x * x == 16);
}
"#,
    );
    let tests = discover(&path);
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[0].settings().backend(), TestBackend::Groth16);
    assert_eq!(tests[1].settings().backend(), TestBackend::Mirage);

    for test in &tests {
        let outcome = run_selected(test);
        assert!(
            matches!(outcome, Outcome::Pass),
            "{} with {} returned {:?}",
            test.name(),
            test.settings().backend(),
            outcome
        );
    }
}

#[test]
fn nested_tuple_passes_full_pipeline() {
    let outcome = run_only(
        "nested_tuple",
        r#"@test t = ([1, 2], (true, 3));
def test_tuple(private (field[2], (bool, u32)) t) {
    assert(t.0[0] == 1);
    assert(t.0[1] == 2);
    assert(t.1.0);
    assert(t.1.1 == 3u32);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn false_private_tuple_assertion_is_assertion_failure() {
    // Linear, all-private: the class of assert that optimization can
    // eliminate (the vacuous-pass hazard) — must FAIL, not pass, for tuple
    // inputs just as for scalars and arrays.
    let outcome = run_only(
        "false_private_tuple",
        r#"@test t = (4, true);
def test_tuple(private (field, bool) t) {
    assert(t.0 == 99);
    assert(t.1);
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
fn public_tuple_passes() {
    // No visibility keyword = public: every tuple leaf goes into the
    // verifier map too.
    let outcome = run_only(
        "public_tuple",
        r#"@test t = (7, 9);
def test_tuple((field, u32) t) {
    assert(t.0 == 7);
    assert(t.1 == 9u32);
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

#[test]
fn chall_lookup_mirage_passes_full_pipeline() {
    // Exercises sample_challenge and value_in_array through the Mirage
    // proof pipeline. Builtin lengths are explicit to avoid generic inference.
    let outcome = run_only(
        "chall_lookup",
        r#"from "EMBED" import value_in_array, sample_challenge;

const field[5] TABLE = [0, 1, 2, 3, 4];

@test(backend = mirage) image = [[1, 6], [7, 2]], bin = [[1, 0], [0, 1]], flat = [1, 0, 0, 1];
def test_binarize(private field[2][2] image, private field[2][2] bin, private field[4] flat) {
    for u32 r in 0..2 {
        for u32 c in 0..2 {
            // each claimed-binary pixel is a bit
            assert(bin[r][c] * (1 - bin[r][c]) == 0);
            // range lookup: bin = 1 -> image in [0,4]; bin = 0 -> image in [4,8]
            field to_lookup = image[r][c] - 4 * (1 - bin[r][c]);
            assert(value_in_array::<5>(to_lookup, TABLE));
        }
    }
    // flat == row-major(bin), checked as a polynomial identity at a
    // verifier challenge; holds for every gamma when consistent
    field gamma = sample_challenge::<8>([...flat, ...bin[0], ...bin[1]]);
    field mut lhs = 0;
    field mut rhs = 0;
    field mut power = 1;
    for u32 i in 0..4 {
        lhs = lhs + flat[i] * power;
        power = power * gamma;
    }
    field mut power2 = 1;
    for u32 r in 0..2 {
        for u32 c in 0..2 {
            rhs = rhs + bin[r][c] * power2;
            power2 = power2 * gamma;
        }
    }
    assert(lhs == rhs);
}
"#,
    );
    assert!(matches!(outcome, Outcome::Pass), "got {:?}", outcome);
}

#[test]
fn chall_circuit_on_groth16_is_backend_error() {
    // Control: a challenge circuit under Groth16 is a BackendError (Bellman
    // rejects round structure at setup) — never Pass, and never a semantic
    // AssertionFailed. x = y makes the identity hold for every challenge, so
    // the pre-check passes and the failure is pinned to the backend.
    let outcome = run_only(
        "chall_groth16",
        r#"from "EMBED" import sample_challenge;

@test(backend = groth16) x = 3, y = 3;
def test_chall(private field x, private field y) {
    field a = sample_challenge::<2>([x, y]);
    assert(a * x == a * y);
}
"#,
    );
    assert!(
        matches!(outcome, Outcome::BackendError(_)),
        "got {:?}",
        outcome
    );
}
