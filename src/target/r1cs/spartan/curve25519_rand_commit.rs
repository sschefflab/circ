//! Export circ R1cs to Spartan with commitment support (Curve25519)
use crate::target::r1cs::*;

use crate::util::timer::print_time;
use libdorian::scalar::Scalar as OriScalar;
use libdorian::{
    Assignment, DensePolynomial, InputsAssignment, Instance,
    NIZKRand, NIZKRandGens, NIZKRandInter,
    PolyCommitment, PolyCommitmentBlinds,
};
use merlin::Transcript;
use std::io;
use std::time::Instant;

use super::spartan_rand::ISpartanCommitProofSystem;
use super::curve25519::int_to_scalar;
use super::curve25519_rand::R1csToSpartan2Round;

use crate::target::r1cs::proof::deserialize_from_file;
use std::path::Path;


/// Spartan proof system using prove_01_commit + prove_1_commit over Curve25519
pub struct SpartanRandCommitCurve25519;

impl ISpartanCommitProofSystem for SpartanRandCommitCurve25519 {
    type VerifierKey = VerifierData;
    type ProverKey = ProverDataSpartanRand;
    type SetupParameter = (NIZKRandGens, Instance);
    type Proof = NIZKRand;
    type DensePoly = DensePolynomial;
    type Commitment = PolyCommitment;
    type CommitmentBlinds = PolyCommitmentBlinds;

    fn prove_fs_inner(
        pk_path: impl AsRef<Path>,
        pp: &Self::SetupParameter,
        commit_input_map: &HashMap<String, Value>,
        plain_input_map: &HashMap<String, Value>,
        wit_poly: Self::DensePoly,
        wit_comm: Self::Commitment,
        wit_blinds: Self::CommitmentBlinds,
    ) -> std::io::Result<Self::Proof> {
        let print_msg = true;
        let (pubinp_len, wit_len, rand_list, precompute, field) = {
            let prover_data: Self::ProverKey = deserialize_from_file(pk_path)?;
            R1csToSpartan2Round::parse_prover_data(&prover_data)
        };

        let mut evaluator = R1csToSpartan2Round::from_prover_data_inner(
            &pubinp_len,
            &wit_len,
            &rand_list,
            &precompute,
            &field
        );
        let (gens, inst) = pp;
        let start = Instant::now();
        let pf = prove_commit(
            &mut evaluator, gens, inst,
            commit_input_map, plain_input_map,
            wit_poly, wit_comm, wit_blinds,
        ).unwrap();
        print_time("Time for Proving (commit)", start.elapsed(), print_msg);
        Ok(pf)
    }

    fn verify(
        pp: &Self::SetupParameter,
        vk: &Self::VerifierKey,
        proof: &Self::Proof,
        inputs_map: &HashMap<String, Value>,
        _print_msg: bool,
    ) -> io::Result<()> {
        let values = vk.eval(inputs_map);
        verify_commit(&values, &pp.0, &pp.1, proof)
    }
}


/// Generate spartan proof using prove_01_commit + prove_1_commit.
///
/// Takes two input maps:
/// - `commit_input_map`: entries whose values form the committed witness (wit0)
/// - `plain_input_map`: the remaining entries
///
/// Internally merges them for the evaluator, and builds the committed witness
/// from `commit_input_map` values.
pub fn prove_commit(
    evaluator: &mut R1csToSpartan2Round,
    gens: &NIZKRandGens,
    inst: &Instance,
    commit_input_map: &HashMap<String, Value>,
    plain_input_map: &HashMap<String, Value>,
    wit_poly: DensePolynomial,
    wit_comm: PolyCommitment,
    wit_blinds: PolyCommitmentBlinds,
) -> io::Result<NIZKRand> {
    let start_whole = Instant::now();
    #[cfg(debug_assertions)]
    assert_eq!(gens.pubinp_len.len(), 2);
    let print_msg = true;

    let commit_size = commit_input_map.len();

    // Merge the two input maps for the evaluator
    let mut full_input_map = plain_input_map.clone();
    full_input_map.extend(commit_input_map.iter().map(|(k, v)| (k.clone(), v.clone())));

    // The evaluator computes the full first-round witness from all inputs.
    // Split it: the first commit_size elements are the committed part (wit0),
    // the remaining elements are the plaintext part (wit1).
    let (inputs, full_wit0) = evaluator.inputs_to_wit0(&full_input_map);
    let full_wit0_bytes = full_wit0.to_bytes_vec();
    assert!(commit_size <= full_wit0_bytes.len(),
        "commit_size ({}) exceeds first-round witness length ({})",
        commit_size, full_wit0_bytes.len());
    let wit0 = Assignment::new(&full_wit0_bytes[..commit_size]).unwrap();
    let wit1 = Assignment::new(&full_wit0_bytes[commit_size..]).unwrap();

    // produce proof
    let mut prover_transcript = Transcript::new(b"nizkrand_example");
    let mut intermediate = NIZKRandInter::new(&inputs);
    NIZKRand::prove_00(inst, &inputs, gens, &mut prover_transcript);
    let rand_len = gens.pubinp_len[1];
    let verifier_rand: Vec<OriScalar> = NIZKRand::prove_01_commit(
        inst,
        &wit0,
        &wit1,
        wit_poly,
        wit_comm,
        wit_blinds,
        rand_len,
        &mut intermediate,
        gens,
        &mut prover_transcript,
    );

    let start = Instant::now();

    let wit2 = evaluator.rand_to_wit1(&verifier_rand);

    print_time("Time for r1cs_to_spartan1,2 (commit)", start.elapsed(), print_msg);
    let pf = NIZKRand::prove_1_commit(
        inst,
        &wit2,
        &mut intermediate,
        gens,
        &mut prover_transcript,
    );
    print_time("Time for whole prove (commit)", start_whole.elapsed(), print_msg);

    Ok(pf)
}

/// Verify spartan proof produced by prove_01_commit + prove_1_commit
pub fn verify_commit(
    values: &Vec<FieldV>,
    gens: &NIZKRandGens,
    inst: &Instance,
    proof: &NIZKRand,
) -> io::Result<()> {
    let print_msg = true;
    let start = Instant::now();
    let mut inp = Vec::new();
    for v in values {
        let scalar = int_to_scalar(&v.i());
        inp.push(scalar.to_bytes());
    }
    let mut inputs = InputsAssignment::new(&inp).unwrap();
    print_time(
        "Time for Process verifier input -- transforming inputs to appropriate form",
        start.elapsed(),
        print_msg,
    );

    let start = Instant::now();
    let mut verifier_transcript = Transcript::new(b"nizkrand_example");
    assert!(proof
        .verify_commit(inst, &mut inputs, &mut verifier_transcript, gens)
        .is_ok());
    print_time("Time for NIZK::verify_commit", start.elapsed(), print_msg);

    Ok(())
}
