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

#[derive(PartialEq, Eq, Debug, Clone, ValueEnum)]
enum PfCurve {
    T256,
    Curve25519,
    T25519,
}

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
    #[arg(long)]
    action: ProofAction,
    #[arg(long, default_value = "curve25519")]
    pfcurve: PfCurve,
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
    let mut opts = Options::parse();

    // Map --pfcurve to the corresponding field modulus, same as circ.rs does.
    match opts.pfcurve {
        PfCurve::Curve25519 => {
            opts.circ.field.custom_modulus = "7237005577332262213973186563042994240857116359379907606001950938285454250989".to_string();
        }
        PfCurve::T256 => {
            opts.circ.field.custom_modulus = "115792089210356248762697446949407573530086143415290314195533631308867097853951".to_string();
        }
        PfCurve::T25519 => {
            opts.circ.field.custom_modulus = "57896044618658097711785492504343953926634992332820282019728792003956564819949".to_string();
        }
    }
    circ::cfg::set(&opts.circ);

    #[cfg(feature = "spartan")]
    match opts.action {
        ProofAction::Prove => {
            let mut prover_input_map = parse_value_map(&std::fs::read(&opts.inputs).unwrap());

            let commit_prefix = &opts.circ.r1cs.commit_input_prefix;

            let key_matches = |k: &str| -> bool {
                !commit_prefix.is_empty()
                    && (k == commit_prefix.as_str()
                        || k.starts_with(&format!("{}.", commit_prefix))
                        || k.starts_with(&format!("{}_", commit_prefix)))
            };

            // commit_size = number of prefix-matching keys in the input map.
            // This is used consistently in both compute_witness_commitment and prove_commit
            // (which derives commit_size from commit_input_map.len()).
            let commit_size = if commit_prefix.is_empty() {
                0
            } else {
                prover_input_map.keys().filter(|k| key_matches(k)).count()
            };

            if commit_size == 0 {
                // No external commitment — plain Dorian prove with zero dummy commitment.
                println!("Dorian Proving (Curve25519), no external commitment");
                let empty: HashMap<String, circ::ir::term::Value> = HashMap::default();
                let (gens, _inst): (NIZKRandGens, Instance) =
                    deserialize_from_file(&opts.pp).unwrap();
                let num_vars_padded: usize = gens.wit_len.iter().sum();
                let scalars = vec![OriScalar::zero(); num_vars_padded];
                let (wit_poly, wit_comm, wit_blinds) = gens.commit_witness(scalars);
                spartan::spartan_rand::prove_commit_fs(
                    &opts.prover_key,
                    &opts.pp,
                    &empty,
                    &prover_input_map,
                    &opts.proof,
                    wit_poly,
                    wit_comm,
                    wit_blinds,
                )
                .unwrap();
            } else {
                println!(
                    "Dorian Proving with commitment (Curve25519), committing '{}' ({} witness elements)",
                    commit_prefix, commit_size
                );

                let (gens, _inst): (NIZKRandGens, Instance) =
                    deserialize_from_file(&opts.pp).unwrap();

                let (wit_poly, wit_comm, wit_blinds) = compute_witness_commitment(
                    &opts.prover_key, &gens, &prover_input_map, commit_size,
                );

                let committed_keys: Vec<String> = prover_input_map
                    .keys()
                    .filter(|k| key_matches(k))
                    .cloned()
                    .collect();
                let mut commit_input_map: HashMap<String, circ::ir::term::Value> = HashMap::default();
                for key in &committed_keys {
                    let val = prover_input_map.remove(key).unwrap();
                    commit_input_map.insert(key.clone(), val);
                }
                println!("Committed inputs: {} keys matching '{}'", commit_input_map.len(), commit_prefix);

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
        }
        ProofAction::Verify => {
            let verifier_input_map = parse_value_map(&std::fs::read(&opts.inputs).unwrap());
            let commit_prefix = &opts.circ.r1cs.commit_input_prefix;
            if commit_prefix.is_empty() {
                println!("Dorian Verifying (Curve25519), no external commitment");
            } else {
                println!("Dorian Verifying with commitment (Curve25519), committed input: '{}'", commit_prefix);
            }
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
