// Inspect and pretty-print serialized R1CS constraints
use circ::target::r1cs::ProverData;
use fxhash::FxHashMap;
use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <prover_data_file>", args[0]);
        eprintln!("Example: {} P", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    println!("Loading ProverData from: {}", path);

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let prover_data: ProverData = bincode::deserialize_from(reader)
        .expect("Failed to deserialize ProverData");

    println!("\n=== R1CS Summary ===");
    println!("Field modulus: {}", prover_data.r1cs.field().modulus());
    println!("Number of variables: {}", prover_data.r1cs.vars().len());
    println!("Number of constraints: {}", prover_data.r1cs.constraints().len());

    println!("\n=== Variables ===");
    for (i, var) in prover_data.r1cs.vars().iter().enumerate() {
        let name = prover_data.r1cs.names().get(var)
            .map(|s| s.as_str())
            .unwrap_or("<unnamed>");
        println!("Var {:3} ({:?}): {}", i, var, name);
    }

    println!("\n=== Constraints ===");
    for (i, (a, b, c)) in prover_data.r1cs.constraints().iter().enumerate() {
        println!("\nConstraint {}:", i);
        print_lc("  A", a, prover_data.r1cs.names());
        print_lc("  B", b, prover_data.r1cs.names());
        print_lc("  C", c, prover_data.r1cs.names());
    }

    println!("\n=== Witness Computation Info ===");
    println!("Number of computation steps: {}", prover_data.precompute.num_steps());
    println!("Number of step arguments: {}", prover_data.precompute.num_step_args());

    Ok(())
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
