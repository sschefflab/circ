//! In-memory compilation helpers.
//!
//! These let callers (such as the unit-test runner) turn a compiled
//! [`Computation`] into the data a proof system needs — without writing files.

use crate::cfg::CircCfg;
use crate::ir::term::Computation;
use crate::target::r1cs::opt::reduce_linearities;
use crate::target::r1cs::trans::to_r1cs;
use crate::target::r1cs::{ProverData, R1csStats, VerifierData};

/// Lower a compiled [`Computation`] to R1CS, run the standard R1CS
/// optimizations, and produce the prover and verifier data used by a proof
/// system's `setup`, `prove`, and `verify`.
///
/// Also returns the final [`R1csStats`], and honors the `--r1cs-profile` flag by
/// printing the R1CS size before and after optimization.
pub fn to_proof_data(cs: &Computation, cfg: &CircCfg) -> (ProverData, VerifierData, R1csStats) {
    let mut r1cs = to_r1cs(cs, cfg);
    if cfg.r1cs.profile {
        println!("Pre-opt  r1cs stats: {:#?}", r1cs.stats());
    }
    r1cs = reduce_linearities(r1cs, cfg);
    if cfg.r1cs.profile {
        println!("Post-opt r1cs stats: {:#?}", r1cs.stats());
    }
    let stats = r1cs.stats().clone();
    let (prover_data, verifier_data) = r1cs.finalize(cs);
    (prover_data, verifier_data, stats)
}
