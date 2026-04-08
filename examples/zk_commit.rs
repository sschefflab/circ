use circ::cfg::{
    clap::{self, Parser, ValueEnum},
    CircOpt,
};
use std::path::PathBuf;

#[cfg(feature = "spartan")]
use circ::ir::term::text::parse_value_map;
#[cfg(feature = "spartan")]
use circ::target::r1cs::spartan;

#[cfg(feature = "spartan")]
use circ::target::r1cs::proof::deserialize_from_file;
#[cfg(feature = "spartan")]
use circ::target::r1cs::spartan::curve25519_rand::R1csToSpartan2Round;
#[cfg(feature = "spartan")]
use libdorian::{
    DensePolynomial, NIZKRandGens, Instance,
    PolyCommitment, PolyCommitmentBlinds,
    scalar::Scalar as OriScalar,
};
#[cfg(feature = "spartan")]
use circ::target::r1cs::ProverDataSpartanRand;
#[cfg(feature = "spartan")]
use fxhash::FxHashMap as HashMap;

#[derive(Debug, Parser)]
#[command(name = "zk_commit", about = "The CirC ZKP runner with commitment support")]
struct Options {
    #[arg(long, default_value = "P")]
    prover_key: PathBuf,
    #[arg(long, default_value = "V")]
    verifier_key: PathBuf,
    #[arg(long, default_value = "SpartanPP")]
    pp: PathBuf,
    #[arg(long, default_value = "pi")]
    proof: PathBuf,
    #[arg(long, default_value = "in")]
    inputs: PathBuf,
    #[arg(long, default_value = "pin")]
    pin: PathBuf,
    #[arg(long, default_value = "vin")]
    vin: PathBuf,
    #[arg(long)]
    action: ProofAction,
    /// Number of first-round witness elements to commit externally
    #[arg(long, default_value = "4")]
    commit_size: usize,
    #[command(flatten)]
    circ: CircOpt,
}

#[derive(PartialEq, Debug, Clone, ValueEnum)]
enum ProofAction {
    Prove,
    Verify,
}

/// Run the evaluator with the full input map to compute the first-round witness,
/// then commit the first `commit_size` elements using the generators from `gens`.
#[cfg(feature = "spartan")]
fn compute_witness_commitment(
    pk_path: &PathBuf,
    gens: &NIZKRandGens,
    input_map: &HashMap<String, circ::ir::term::Value>,
    commit_size: usize,
) -> (DensePolynomial, PolyCommitment, PolyCommitmentBlinds) {
    let (pubinp_len, wit_len, rand_list, precompute, field) = {
        let prover_data: ProverDataSpartanRand = deserialize_from_file(pk_path).unwrap();
        R1csToSpartan2Round::parse_prover_data(&prover_data)
    };
    let mut evaluator = R1csToSpartan2Round::from_prover_data_inner(
        &pubinp_len, &wit_len, &rand_list, &precompute, &field,
    );

    let (_inputs, full_wit0) = evaluator.inputs_to_wit0(input_map);
    let full_wit0_bytes = full_wit0.to_bytes_vec();
    assert!(commit_size <= full_wit0_bytes.len(),
        "commit_size ({}) exceeds first-round witness length ({})",
        commit_size, full_wit0_bytes.len());

    // Pad to full witness size: first commit_size elements from wit0, rest zeros
    let num_vars_padded: usize = gens.wit_len.iter().sum();
    let mut padded_scalars = vec![OriScalar::zero(); num_vars_padded];
    for (i, b) in full_wit0_bytes[..commit_size].iter().enumerate() {
        let ct = OriScalar::from_bytes(b);
        assert!(ct.is_some().unwrap_u8() == 1, "invalid scalar");
        padded_scalars[i] = ct.unwrap();
    }

    gens.commit_witness(padded_scalars)
}

fn main() {
    env_logger::Builder::from_default_env()
        .format_level(false)
        .format_timestamp(None)
        .init();
    let opts = Options::parse();
    circ::cfg::set(&opts.circ);

    #[cfg(feature = "spartan")]
    match opts.action {
        ProofAction::Prove => {
            let mut prover_input_map = parse_value_map(&std::fs::read(&opts.inputs).unwrap());
            let commit_size = opts.commit_size;
            println!("Dorian Proving with commitment (Curve25519), commit_size={}", commit_size);

            // Load setup parameters
            let (gens, _inst): (NIZKRandGens, Instance) =
                deserialize_from_file(&opts.pp).unwrap();

            // Compute the commitment from the actual evaluator witness
            let (wit_poly, wit_comm, wit_blinds) = compute_witness_commitment(
                &opts.prover_key, &gens, &prover_input_map, commit_size,
            );

            // Split input map: first commit_size keys (sorted) go to commit_input_map
            let mut keys: Vec<String> = prover_input_map.keys().cloned().collect();
            keys.sort();
            assert!(commit_size <= keys.len(),
                "commit_size ({}) exceeds number of inputs ({})", commit_size, keys.len());
            let committed_keys: Vec<String> = keys[..commit_size].to_vec();
            let mut commit_input_map: HashMap<String, circ::ir::term::Value> = HashMap::default();
            for key in &committed_keys {
                let val = prover_input_map.remove(key).unwrap();
                commit_input_map.insert(key.clone(), val);
            }
            println!("Committed inputs: {:?}", committed_keys);

            // Prove: commit_input_map has committed entries, prover_input_map has the rest
            spartan::spartan_rand::prove_commit_fs(
                &opts.prover_key,
                &opts.pp,
                &commit_input_map,
                &prover_input_map,
                &opts.proof,
                wit_poly,
                wit_comm,
                wit_blinds,
            )
            .unwrap();
        }
        ProofAction::Verify => {
            let verifier_input_map = parse_value_map(&std::fs::read(&opts.inputs).unwrap());
            println!("Dorian Verifying with commitment (Curve25519)");
            spartan::spartan_rand::verify_commit_fs(
                &opts.verifier_key,
                &opts.pp,
                &verifier_input_map,
                &opts.proof,
            )
            .unwrap();
        }
    }

    #[cfg(not(feature = "spartan"))]
    panic!("Missing feature: spartan");
}
