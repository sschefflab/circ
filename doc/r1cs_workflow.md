# R1CS Workflow with Spartan Backend

This guide explains how to compile ZoKrates programs to R1CS, inspect the constraints, and use the serialized R1CS for proof generation.

## Overview

The Spartan backend provides explicit R1CS serialization, allowing you to:
1. **Compile once** - Generate R1CS from ZoKrates source
2. **Inspect** - View human-readable constraint equations
3. **Reuse** - Load the serialized R1CS for multiple proof generations

## Prerequisites

### Configure Features

```bash
./driver.py -F zok r1cs spartan smt zokc
```

This enables:
- `zok` - ZoKrates/Z# frontend
- `zokc` - ZoKratesCurly / modern ZoKrates frontend
- `r1cs` - R1CS backend
- `spartan` - Spartan proof system (uses Curve25519 scalar field)
- `smt` - SMT backend (required by ZoKrates frontend)

### Build

```bash
./driver.py -b
```

This compiles three tools you'll need:
- `target/release/examples/circ` - Main compiler
- `target/release/examples/r1cs_inspect` - R1CS inspector
- Spartan proof system libraries

## Step 1: Compile ZoKrates to Serialized R1CS

Compile your ZoKrates program and save the R1CS:

```bash
./target/release/examples/circ examples/ZoKrates/pf/3_plus.zok r1cs \
    --action spartan-setup \
    --prover-key P \
    --verifier-key V
```
**Note: for modern ZoKrates, you need `language --zsharp-curly` before `r1cs`

### What This Does

1. **Frontend**: Parses the `.zok` file
2. **IR Generation**: Converts to CirC's intermediate representation
3. **Optimization**: Applies passes like:
   - Constant folding
   - Tuple elimination
   - Array elimination (oblivious and linear-scan)
   - Linearity reduction
4. **R1CS Conversion**: Lowers IR to R1CS constraints
5. **Serialization**: Saves two files using bincode:
   - **`P`** - ProverData (full R1CS + witness computation)
   - **`V`** - VerifierData (instance computation + metadata)

### Output Files

#### `P` (ProverData) - ~Several KB
Contains:
- `r1cs: R1csFinal`
  - Field modulus (Curve25519 scalar field)
  - Variables: `Vec<Var>`
  - Constraints: `Vec<(Lc, Lc, Lc)>` where each constraint is `A * B = C`
  - Variable names: `HashMap<Var, String>`
- `precompute: StagedWitComp` - Instructions for computing witness from inputs

#### `V` (VerifierData) - Smaller
Contains:
- `precompute: StagedWitComp` - Instructions for computing public inputs/outputs
- `num_commitments: usize` - Number of witness commitments

### Expected Output

```
Compiling
Running frontend
Optimizing IR
Running backend
Running r1cs optimizations
Final r1cs: 8 constraints, 15 variables, 32 entries, 0 rounds
Final Witext steps: 12, arguments: 24
```

## Step 2: Inspect the R1CS Constraints

View the constraints in human-readable format:

```bash
./target/release/examples/r1cs_inspect P
```

### Output Format

#### R1CS Summary
```
=== R1CS Summary ===
Field modulus: 7237005577332262213973186563042994240857116359379907606001950938285454250989
Number of variables: 15
Number of constraints: 8
```

#### Variables with Names
```
=== Variables ===
Var   0 (Inst(0)): main.x
Var   1 (Inst(1)): main.return
Var   2 (FinalWit(0)): v_42
Var   3 (FinalWit(1)): v_43
...
```

Variable types:
- `Inst` - Instance variables (public inputs/outputs)
- `FinalWit` - Final witness variables (private inputs and intermediates)
- `RoundWit` - Round witness (for interactive proofs)
- `Chall` - Challenge variables (for interactive proofs)
- `CWit` - Committed witness

#### Constraints as Equations
```
=== Constraints ===

Constraint 0:
  A: main.x
  B: main.x
  C: v_42

Constraint 1:
  A: 2*v_42 + 3*main.x
  B: 1
  C: main.return
...
```

Each constraint represents: `A * B = C`

#### Witness Computation Stats
```
=== Witness Computation Info ===
Number of computation steps: 12
Number of step arguments: 24
```

## Step 3: Use R1CS for Proof Generation

The serialized R1CS in file `P` can be loaded for proof generation. The Spartan module provides functions to work with the saved data.

### In Rust Code

```rust
use circ::target::r1cs::spartan::{prove, verify, read_prover_data};
use circ::ir::term::Value;
use fxhash::FxHashMap as HashMap;

// Load the serialized ProverData
let prover_data = read_prover_data("P")?;

// Prepare witness inputs (maps variable names to values)
let mut inputs: HashMap<String, Value> = HashMap::default();
// Add your witness values here

// Generate proof
let (gens, inst, proof) = prove("P", &inputs)?;

// Verify proof
verify("V", &inputs, &gens, &inst, proof)?;
```

### Input File Format

For the command-line tools, inputs are provided in S-expression format (`.pin` for prover, `.vin` for verifier).

Example `3_plus.zok.pin`:
```lisp
(let (
    (x #x04)
)
    false
)
```

This sets the private input `x` to `0x04` (4 in decimal).

## Complete Example Workflow

### Example Program: Simple Multiplication

Create `example.zok`:
```zok
def main(private field x, private field y) -> field:
    return x * y
```

### 1. Compile to R1CS

```bash
./target/release/examples/circ example.zok r1cs \
    --action spartan-setup \
    --prover-key P_example \
    --verifier-key V_example
```

### 2. Inspect Constraints

```bash
./target/release/examples/r1cs_inspect P_example
```

You should see constraints like:
```
Constraint 0:
  A: main.x
  B: main.y
  C: main.return
```

This encodes the multiplication: `x * y = return`

### 3. The R1CS is Now Saved

The files `P_example` and `V_example` contain the serialized R1CS. These can be:
- Loaded for proof generation (via Spartan API)
- Archived for later use
- Transferred to other systems
- Used with custom proof backends

## Advanced: Custom Output Locations

```bash
./target/release/examples/circ my_program.zok r1cs \
    --action spartan-setup \
    --prover-key /path/to/prover_data.bin \
    --verifier-key /path/to/verifier_data.bin
```

## Comparing with Raw Output

To see the difference between human-readable and raw output:

```bash
# Raw debug format (variable IDs only)
./target/release/examples/zxc examples/ZoKrates/pf/3_plus.zok --action count 2>&1 | less

# Human-readable with variable names
./target/release/examples/r1cs_inspect P
```

## Field Compatibility

The Spartan backend uses the **Curve25519 scalar field**:
```
Modulus: 7237005577332262213973186563042994240857116359379907606001950938285454250989
```

If your R1CS uses a different field, you'll get an assertion error. Most ZoKrates programs work with this field by default.

## Optimization Control

### Skip Linearity Reduction

To see the unoptimized R1CS (more constraints, easier to trace to source):

```bash
./target/release/examples/zxc example.zok -L --action count
```

The `-L` flag skips the linearity reduction pass.

### View Statistics Only

To compile without saving files:

```bash
./target/release/examples/circ example.zok r1cs --action count
```

This prints statistics but doesn't write `P` and `V` files.

## File Sizes

Expected file sizes for typical programs:
- Simple programs (3-10 constraints): P ~10-50 KB, V ~5-20 KB
- Medium programs (100-1000 constraints): P ~100-500 KB, V ~50-200 KB
- Large programs (10000+ constraints): P ~5-50 MB, V ~1-10 MB

File size depends on:
- Number of constraints
- Number of variables
- Complexity of witness computation

## Troubleshooting

### "Field modulus mismatch"
Your program uses a different field than Curve25519. Check the field configuration or use the Bellman backend instead.

### "Missing value in R1cs::eval"
The input file is missing a required witness variable. Check that all private inputs are provided in the `.pin` file.

### "Failed to deserialize ProverData"
The file may be corrupted or was generated with incompatible CirC versions. Recompile from source.

## Next Steps

- See [zkp.md](./zkp.md) for the Bellman/Groth16 workflow
- For custom backends, deserialize `ProverData` and access the R1CS directly
- Export R1CS to other formats by writing a converter that reads the ProverData structure

## API Reference

Key types in `circ::target::r1cs`:

```rust
pub struct ProverData {
    pub r1cs: R1csFinal,
    pub precompute: StagedWitComp,
}

pub struct R1csFinal {
    // Accessor methods:
    pub fn field(&self) -> &FieldT;
    pub fn vars(&self) -> &Vec<Var>;
    pub fn constraints(&self) -> &Vec<(Lc, Lc, Lc)>;
    pub fn names(&self) -> &HashMap<Var, String>;
}

pub struct Lc {
    // Accessor methods:
    pub fn constant(&self) -> &FieldV;
    pub fn monomials(&self) -> &HashMap<Var, FieldV>;
}
```
