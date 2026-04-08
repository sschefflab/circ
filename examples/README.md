# CirC ZKP Examples

## `zk.rs` — Generate and verify proofs

Supports proof backends: `groth16`, `mirage`, `spartan`, `dorian`.

The commands below use `poly_mult.zok` with the Dorian backend on Curve25519 as an example.
Substitute your own circuit path, input files, backend, and curve as needed.

**Step 1: Setup** (compile circuit and generate proving/verifying keys)
```
./target/release/examples/circ <circuit>.zok r1cs \
  --action setup --proof-impl <backend> --pfcurve <curve>
```
```
# Example
./target/release/examples/circ examples/ZoKrates/pf/chall/poly_mult.zok r1cs \
  --action setup --proof-impl dorian --pfcurve curve25519
```

**Step 2: Prove**
```
./target/release/examples/zk \
  --inputs <circuit>.zok.pin \
  --action prove --proof-impl <backend> --pfcurve <curve>
```
```
# Example
./target/release/examples/zk \
  --inputs examples/ZoKrates/pf/chall/poly_mult_curve25519.zok.pin \
  --action prove --proof-impl dorian --pfcurve curve25519
```

**Step 3: Verify**
```
./target/release/examples/zk \
  --inputs <circuit>.zok.vin \
  --action verify --proof-impl <backend> --pfcurve <curve>
```
```
# Example
./target/release/examples/zk \
  --inputs examples/ZoKrates/pf/chall/poly_mult_curve25519.zok.vin \
  --action verify --proof-impl dorian --pfcurve curve25519
```

---

## `zk_commit.rs` — Prove with an external witness commitment

Like `zk.rs`, but commits to the first `--commit-size` elements of the witness before proving.
This demonstrates how to construct a proof relative to an externally provided commitment
(e.g., a commitment made by another party). Currently only supports **Dorian + Curve25519**.

The commands below use `poly_mult.zok` as an example. Substitute your own circuit and input files as needed.

**Step 1: Setup** (same as `zk.rs`, must use `--proof-impl dorian --pfcurve curve25519`)
```
./target/release/examples/circ <circuit>.zok r1cs \
  --action setup --proof-impl dorian --pfcurve curve25519
```
```
# Example
./target/release/examples/circ examples/ZoKrates/pf/chall/poly_mult.zok r1cs \
  --action setup --proof-impl dorian --pfcurve curve25519
```

**Step 2: Prove** (commits to first `--commit-size` witness elements; default is 4)
```
./target/release/examples/zk_commit \
  --inputs <circuit>.zok.pin \
  --action prove --commit-size <n>
```
```
# Example
./target/release/examples/zk_commit \
  --inputs examples/ZoKrates/pf/chall/poly_mult_curve25519.zok.pin \
  --action prove --commit-size 4
```

**Step 3: Verify**
```
./target/release/examples/zk_commit \
  --inputs <circuit>.zok.vin \
  --action verify
```
```
# Example
./target/release/examples/zk_commit \
  --inputs examples/ZoKrates/pf/chall/poly_mult_curve25519.zok.vin \
  --action verify
```
