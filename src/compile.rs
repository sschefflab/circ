//! In-memory compilation helpers.
//!
//! These let callers (such as the unit-test runner) turn a compiled
//! [`Computation`] into the data a proof system needs — without writing files.

use crate::cfg::CircCfg;
use crate::ir::term::Computation;
use crate::target::r1cs::opt::reduce_linearities;
use crate::target::r1cs::trans::to_r1cs;
use crate::target::r1cs::{ProverData, VerifierData};

/// Lower a compiled [`Computation`] to R1CS, run the standard R1CS
/// optimizations, and produce the prover and verifier data used by a proof
/// system's `setup`, `prove`, and `verify`.
///
/// This is the same work the `circ` binary does inline, but it returns the
/// structs in memory instead of serializing them to disk.
pub fn to_proof_data(cs: &Computation, cfg: &CircCfg) -> (ProverData, VerifierData) {
    let mut r1cs = to_r1cs(cs, cfg);
    r1cs = reduce_linearities(r1cs, cfg);
    r1cs.finalize(cs)
}