//! In-memory compilation helpers.
//!
//! These let callers (such as the unit-test runner) turn a compiled
//! [`Computation`] into the data a proof system needs — without writing files.

use crate::cfg::CircCfg;
use crate::ir::opt::{opt, Opt};
use crate::ir::term::{Computation, Computations};
use crate::target::r1cs::opt::reduce_linearities;
use crate::target::r1cs::trans::to_r1cs;
use crate::target::r1cs::{ProverData, R1csStats, VerifierData};

/// Run the canonical proof-mode IR optimization pipeline on a set of
/// [`Computations`].
///
/// This is the single owner of the pass list the CLI driver (`examples/circ.rs`
/// `Mode::Proof`) and the unit-test runner both use, so they cannot drift. The
/// passes are backend-independent (Groth16, Mirage, and Spartan all consume the
/// same optimized IR); the only configurable input is
/// [`cfg.ir.fits_in_bits_ip`](crate::cfg). This must run before [`to_proof_data`]:
/// R1CS lowering embeds scalar sorts only, so array/tuple terms have to be
/// scalarized and eliminated here first.
pub fn opt_for_proof(cs: Computations, cfg: &CircCfg) -> Computations {
    let mut opts = vec![
        Opt::ConstantFold(Box::new([])),
        Opt::DeskolemizeWitnesses,
        Opt::ScalarizeVars,
        Opt::Flatten,
        Opt::Sha,
        Opt::ConstantFold(Box::new([])),
        Opt::ParseCondStores,
        // Tuples must be eliminated before oblivious array elim
        Opt::ConstantFold(Box::new([])),
        Opt::Obliv,
        // The obliv elim pass produces more tuples, that must be eliminated
        Opt::SetMembership,
        Opt::PersistentRam,
        Opt::VolatileRam,
    ];
    if cfg.ir.fits_in_bits_ip {
        opts.push(Opt::FitsInBitsIp);
    }
    opts.extend([
        Opt::SkolemizeChallenges,
        Opt::ScalarizeVars,
        Opt::ConstantFold(Box::new([])),
        Opt::Obliv,
        Opt::LinearScan,
        // The linear scan pass produces more tuples, that must be eliminated
        Opt::Tuple,
        Opt::Flatten,
        Opt::ConstantFold(Box::new([])),
    ]);
    opt(cs, opts)
}

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
