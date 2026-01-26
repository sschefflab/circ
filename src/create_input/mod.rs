//! Create prover inputs and verifier inputs

use crate::cfg::clap::{self, ValueEnum};

#[derive(PartialEq, Eq, Debug, Clone, ValueEnum)]
/// Curve for Spartan
pub enum PfCurve {
    /// Curve T256
    T256,
    /// Curve25519
    Curve25519,
    /// Curve T25519
    T25519,
}
