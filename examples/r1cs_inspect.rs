// Inspect and pretty-print serialized R1CS constraints
use circ::target::r1cs::{ProverData, R1csFinal};
use circ::target::r1cs::wit_comp::StagedWitComp;
use fxhash::FxHashMap;
use std::env;
use std::fs::File;
use std::io::BufReader;

#[derive(Debug)]
enum Format {
    ProverData,
    #[cfg(feature = "bellman")]
    Bellman,
    #[cfg(feature = "spartan")]
    Dorian,
}

fn parse_format(s: &str) -> Format {
    match s {
        "bellman" => {
            #[cfg(feature = "bellman")]
            return Format::Bellman;
            #[cfg(not(feature = "bellman"))]
            {
                eprintln!("Error: bellman format requires the 'bellman' feature");
                std::process::exit(1);
            }
        }
        "dorian" => {
            #[cfg(feature = "spartan")]
            return Format::Dorian;
            #[cfg(not(feature = "spartan"))]
            {
                eprintln!("Error: dorian format requires the 'spartan' feature");
                std::process::exit(1);
            }
        }
        "prover-data" => Format::ProverData,
        other => {
            eprintln!("Error: unknown format '{}'. Valid formats: prover-data, bellman, dorian", other);
            std::process::exit(1);
        }
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    let (path, format) = match args.len() {
        2 => (&args[1], Format::ProverData),
        4 if args[2] == "--format" => (&args[1], parse_format(&args[3])),
        4 if args[1] == "--format" => (&args[3], parse_format(&args[2])),
        _ => {
            eprintln!("Usage: {} <prover_data_file> [--format prover-data|bellman|dorian]", args[0]);
            eprintln!("Example: {} P", args[0]);
            eprintln!("Example: {} P --format bellman", args[0]);
            eprintln!("Example: {} P --format dorian", args[0]);
            std::process::exit(1);
        }
    };

    match format {
        Format::ProverData => {
            println!("Loading ProverData from: {}", path);
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let prover_data: ProverData = bincode::deserialize_from(reader)
                .expect("Failed to deserialize ProverData");
            inspect_r1cs(&prover_data.r1cs, &prover_data.precompute);
        }
        #[cfg(feature = "bellman")]
        Format::Bellman => {
            use bls12_381::Bls12;
            use circ::target::r1cs::bellman::ProvingKey;
            println!("Loading Bellman ProvingKey from: {}", path);
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let pk: ProvingKey<Bls12> = bincode::deserialize_from(reader)
                .expect("Failed to deserialize Bellman ProvingKey");
            inspect_r1cs(&pk.prover_data().r1cs, &pk.prover_data().precompute);
        }
        #[cfg(feature = "spartan")]
        Format::Dorian => {
            use circ::target::r1cs::ProverDataSpartanRand;
            println!("Loading Dorian ProverDataSpartanRand from: {}", path);
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let pd: ProverDataSpartanRand = bincode::deserialize_from(reader)
                .expect("Failed to deserialize ProverDataSpartanRand");
            println!("\n=== Dorian Summary ===");
            println!("Public input + verifier randomness lengths: {:?}", pd.pubinp_len);
            println!("Witness lengths: {:?}", pd.wit_len);
            inspect_r1cs(&pd.r1cs, &pd.precompute);
        }
    }

    Ok(())
}

fn inspect_r1cs(r1cs: &R1csFinal, precompute: &StagedWitComp) {
    println!("\n=== R1CS Summary ===");
    println!("Field modulus: {}", r1cs.field().modulus());
    println!("Number of variables: {}", r1cs.vars().len());
    println!("Number of constraints: {}", r1cs.constraints().len());

    println!("\n=== Variables ===");
    for (i, var) in r1cs.vars().iter().enumerate() {
        let name = r1cs.names().get(var)
            .map(|s| s.as_str())
            .unwrap_or("<unnamed>");
        println!("Var {:3} ({:?}): {}", i, var, name);
    }

    println!("\n=== Constraints ===");
    for (i, (a, b, c)) in r1cs.constraints().iter().enumerate() {
        println!("\nConstraint {}:", i);
        print_lc("  A", a, r1cs.names());
        print_lc("  B", b, r1cs.names());
        print_lc("  C", c, r1cs.names());
    }

    println!("\n=== Witness Computation Info ===");
    println!("Number of computation steps: {}", precompute.num_steps());
    println!("Number of step arguments: {}", precompute.num_step_args());
}

fn print_lc(label: &str, lc: &circ::target::r1cs::Lc, names: &FxHashMap<circ::target::r1cs::Var, String>) {
    use std::fmt::Write;
    let mut s = String::new();

    // Print constant
    if lc.constant().i() != 0 {
        write!(&mut s, "{}", lc.constant().i()).unwrap();
    }

    // Print monomials
    for (var, coeff) in lc.monomials() {
        if !s.is_empty() {
            s.push_str(" + ");
        }
        let var_name = names.get(var)
            .map(|s| s.as_str())
            .unwrap_or("?");

        if coeff.i() == 1 {
            write!(&mut s, "{}", var_name).unwrap();
        } else {
            write!(&mut s, "{}*{}", coeff.i(), var_name).unwrap();
        }
    }

    if s.is_empty() {
        s.push('0');
    }

    println!("{}: {}", label, s);
}
