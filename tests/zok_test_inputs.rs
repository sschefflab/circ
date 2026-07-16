//! The input contract of `ZSharpCurlyFE::eval_test_inputs`: which `@test`
//! annotations are accepted, how their input expressions are typed and
//! evaluated, and which malformed annotations are rejected (and how).
//!
//! Each test writes its program to a uniquely named file in the OS temp dir,
//! so nothing touches the source tree and tests can run in parallel.

#![cfg(all(feature = "smt", feature = "zokc"))]

use circ::front::zsharpcurly::{TestCase, ZSharpCurlyFE};
use circ::front::Mode;
use circ::ir::term::Value;
use std::sync::Once;

static INIT: Once = Once::new();

/// A temp file that is deleted on drop — even when the test panics, so
/// failing tests don't leave files behind.
struct TempZok(std::path::PathBuf);

impl Drop for TempZok {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write `src` to a unique temp file and run eval_test_inputs on it.
fn eval(test_name: &str, src: &str) -> Result<Vec<TestCase>, String> {
    // The CirC config is process-global and can only be set once.
    INIT.call_once(circ::cfg::set_default);
    // The process id keeps concurrent `cargo test` processes from colliding
    // on the same paths; the test name separates threads within a process.
    let path = std::env::temp_dir().join(format!(
        "zok_test_inputs_{}_{}.zok",
        std::process::id(),
        test_name
    ));
    std::fs::write(&path, src).unwrap();
    let guard = TempZok(path);
    ZSharpCurlyFE::eval_test_inputs(guard.0.clone(), Mode::Proof)
}

/// Assert a Value is a field element with the given integer value.
fn assert_field(v: &Value, expected: u64) {
    match v {
        Value::Field(f) => assert_eq!(f.i(), expected),
        other => panic!("expected field {}, got {:?}", expected, other),
    }
}

/// Assert a Value is a bit-vector of the given width and integer value.
fn assert_uint(v: &Value, width: usize, expected: u64) {
    match v {
        Value::BitVector(b) => {
            assert_eq!(b.width(), width);
            assert_eq!(*b.uint(), expected);
        }
        other => panic!("expected u{} {}, got {:?}", width, expected, other),
    }
}

#[test]
fn constant_arithmetic_evaluates() {
    let tests = eval(
        "constant_arithmetic",
        r#"@test y = 3 * 3;
def test_sq(private field y) -> field {
    return y;
}
"#,
    )
    .unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "test_sq");
    assert_eq!(tests[0].inputs.len(), 1);
    assert_eq!(tests[0].inputs[0].name, "y");
    assert_eq!(tests[0].inputs[0].source, "3 * 3");
    assert_field(&tests[0].inputs[0].value, 9);
}

#[test]
fn named_constant_resolves() {
    let tests = eval(
        "named_constant",
        r#"const field C = 7;

@test x = C + 1;
def test_c(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap();
    assert_field(&tests[0].inputs[0].value, 8);
}

#[test]
fn const_function_call_evaluates() {
    // The const evaluator supports calls to (non-generic, constant-input)
    // functions; "non-constant" means evaluator-rejected, not "contains a
    // call".
    let tests = eval(
        "const_fn_call",
        r#"def sq(field a) -> field {
    return a * a;
}

@test y = sq(3);
def test_sq(private field y) -> field {
    return y;
}
"#,
    )
    .unwrap();
    assert_field(&tests[0].inputs[0].value, 9);
}

#[test]
fn all_scalar_types_evaluate() {
    let tests = eval(
        "all_scalars",
        r#"@test b = true, n8 = 4u8, n16 = 4u16, n32 = 4u32, n64 = 4u64, f = 5f;
def test_scalars(private bool b, private u8 n8, private u16 n16, private u32 n32, private u64 n64, private field f) -> bool {
    return b;
}
"#,
    )
    .unwrap();
    let inputs = &tests[0].inputs;
    assert_eq!(inputs.len(), 6);
    assert!(matches!(inputs[0].value, Value::Bool(true)));
    assert_uint(&inputs[1].value, 8, 4);
    assert_uint(&inputs[2].value, 16, 4);
    assert_uint(&inputs[3].value, 32, 4);
    assert_uint(&inputs[4].value, 64, 4);
    assert_field(&inputs[5].value, 5);
}

#[test]
fn unsuffixed_literal_typed_by_parameter() {
    // The key typing rule: a bare `2` binding to a u32 parameter must come
    // back as a 32-bit bit-vector, not as a field element.
    let tests = eval(
        "unsuffixed_u32",
        r#"@test x = 2;
def test_u(private u32 x) -> u32 {
    return x;
}
"#,
    )
    .unwrap();
    assert_uint(&tests[0].inputs[0].value, 32, 2);
}

#[test]
fn bare_test_zero_params_ok() {
    let tests = eval(
        "bare_zero_params",
        r#"@test;
def test_noargs() -> bool {
    return true;
}
"#,
    )
    .unwrap();
    assert_eq!(tests[0].name, "test_noargs");
    assert!(tests[0].inputs.is_empty());
}

#[test]
fn bare_test_with_params_rejected() {
    let err = eval(
        "bare_with_params",
        r#"@test;
def test_missing(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("missing a value for parameter x"), "{}", err);
}

#[test]
fn missing_input_rejected() {
    let err = eval(
        "missing_input",
        r#"@test a = 1;
def test_two(private field a, private field b) -> field {
    return a;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("missing a value for parameter b"), "{}", err);
}

#[test]
fn unknown_input_rejected() {
    let err = eval(
        "unknown_input",
        r#"@test a = 1, nope = 2;
def test_one(private field a) -> field {
    return a;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("nope does not match any parameter"), "{}", err);
}

#[test]
fn duplicate_input_rejected() {
    let err = eval(
        "duplicate_input",
        r#"@test a = 1, a = 2;
def test_dup(private field a) -> field {
    return a;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("duplicate @test input a"), "{}", err);
}

#[test]
fn non_constant_expression_rejected() {
    let err = eval(
        "non_constant",
        r#"@test x = undefined_thing;
def test_bad(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("undefined_thing"), "{}", err);
}

#[test]
fn array_parameter_rejected() {
    let err = eval(
        "array_param",
        r#"@test xs = [1, 2];
def test_arr(private field[2] xs) -> field {
    return xs[0];
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("non-scalar"), "{}", err);
}

#[test]
fn scalar_type_alias_accepted() {
    // A `type` alias for a scalar resolves to a scalar Ty and must be
    // accepted, even though it parses as a named (non-Basic) type.
    let tests = eval(
        "scalar_alias",
        r#"type Word = u32;

@test x = 2;
def test_alias(private Word x) -> Word {
    return x;
}
"#,
    )
    .unwrap();
    assert_uint(&tests[0].inputs[0].value, 32, 2);
}

#[test]
fn generic_test_function_rejected() {
    let err = eval(
        "generic_fn",
        r#"@test x = 1;
def test_gen<N>(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("cannot be generic"), "{}", err);
}

#[test]
fn type_mismatch_rejected() {
    // `true` cannot bind to a field parameter; the diagnostic must name
    // both the expected and the actual type.
    let err = eval(
        "type_mismatch",
        r#"@test x = true;
def test_ty(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("expected type field"), "{}", err);
    assert!(err.contains("bool"), "{}", err);
}

#[test]
fn annotation_sees_later_declarations() {
    // Annotation expressions have module-wide visibility: they are
    // evaluated after every declaration is processed, so forward references
    // to constants and functions declared later in the file work.
    let tests = eval(
        "decl_order",
        r#"@test x = LATER + double(2);
def test_order(private field x) {
    assert(x == 14);
}

const field LATER = 10;

def double(field n) -> field {
    return n + n;
}
"#,
    )
    .unwrap();
    assert_field(&tests[0].inputs[0].value, 14);
}

#[test]
fn call_arguments_not_typed_by_parameter() {
    // Known limit: parameter-directed literal typing does not descend into
    // call arguments, so the bare `2` defaults to field and clashes with
    // id's u32 parameter...
    let err = eval(
        "call_arg_bare",
        r#"def id(u32 x) -> u32 {
    return x;
}

@test y = id(2);
def test_id(private u32 y) {
    assert(y == 2u32);
}
"#,
    )
    .unwrap_err();
    assert!(err.contains("mismatch"), "{}", err);

    // ...suffixing the literal is the supported form.
    let tests = eval(
        "call_arg_suffixed",
        r#"def id(u32 x) -> u32 {
    return x;
}

@test y = id(2u32);
def test_id(private u32 y) {
    assert(y == 2u32);
}
"#,
    )
    .unwrap();
    assert_uint(&tests[0].inputs[0].value, 32, 2);
}

#[test]
fn inputs_returned_in_parameter_order() {
    // Annotation lists b before a; results follow the signature: a, then b.
    let tests = eval(
        "param_order",
        r#"@test b = 2, a = 1;
def test_order(private field a, private field b) -> field {
    return a;
}
"#,
    )
    .unwrap();
    assert_eq!(tests[0].inputs[0].name, "a");
    assert_field(&tests[0].inputs[0].value, 1);
    assert_eq!(tests[0].inputs[1].name, "b");
    assert_field(&tests[0].inputs[1].value, 2);
}

#[test]
fn visibility_captured_per_parameter() {
    // private -> public: false; public -> true; no keyword defaults to
    // public, matching the frontend's interpret_visibility.
    let tests = eval(
        "visibility",
        r#"@test a = 1, b = 2, c = 3;
def test_vis(private field a, public field b, field c) {
    assert(a + b + c == 6);
}
"#,
    )
    .unwrap();
    let inputs = &tests[0].inputs;
    assert!(!inputs[0].public, "private a");
    assert!(inputs[1].public, "public b");
    assert!(inputs[2].public, "unannotated c defaults to public");
}

#[test]
fn has_return_exposed() {
    // Runners use this to tell self-checking assert-style tests (no return
    // type) from return-typed ones (unsupported in the MVP).
    let tests = eval(
        "has_return",
        r#"@test x = 1;
def test_assert_style(private field x) {
    assert(x == 1);
}

@test y = 2;
def test_return_style(private field y) -> field {
    return y;
}
"#,
    )
    .unwrap();
    assert!(!tests[0].has_return, "assert-style test has no return type");
    assert!(tests[1].has_return, "return-typed test is flagged");
}

#[test]
fn plain_functions_skipped() {
    let tests = eval(
        "plain_skipped",
        r#"def helper(field n) -> field {
    return n + 1;
}

@test x = 1;
def test_only(private field x) -> field {
    return x;
}
"#,
    )
    .unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "test_only");
}

#[test]
fn malformed_file_panics() {
    // Documents current behavior: parse errors are panics inside the
    // frontend (via ZLoad), not Errs. Runners must catch_unwind (as
    // examples/ztest.rs does) until a fallible loader API exists.
    let result = std::panic::catch_unwind(|| eval("malformed", "def broken( {"));
    assert!(result.is_err());
}
