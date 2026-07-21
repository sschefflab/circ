//! Utilities for interpretting zsharp

use super::term::{Ty, T};
use crate::ir::term::*;
use fxhash::FxHashMap as HashMap;

/// Given
/// * a variable name,
/// * a variable type, and
/// * a map from delimited names (e.g., "x", "x.0", "x.field_name") to values
///
/// computes a [T] (of the given type) that contains only constants. These constants are extracted
/// from the map
pub fn extract(
    name: &str,
    ty: &Ty,
    scalar_input_values: &mut HashMap<String, Value>,
) -> Result<T, String> {
    match ty {
        Ty::Bool | Ty::Field | Ty::Uint(..) | Ty::Integer => {
            let ir_val = scalar_input_values
                .remove(name)
                .ok_or_else(|| format!("Could not find scalar variable {name} in the input map"))?;
            Ok(T::new(ty.clone(), const_(ir_val)))
        }
        Ty::Array(elem_count, elem_ty) => T::new_array(
            (0..*elem_count)
                .map(|i| extract(&format!("{name}.{i}"), elem_ty, scalar_input_values))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Ty::MutArray(elem_count) => T::new_array(
            (0..*elem_count)
                .map(|i| extract(&format!("{name}.{i}"), &Ty::Field, scalar_input_values))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Ty::Struct(s_name, fields) => Ok(T::new_struct(
            s_name.clone(),
            fields
                .fields()
                .map(|(f_name, f_ty)| -> Result<(String, T), String> {
                    Ok((
                        f_name.clone(),
                        extract(&format!("{name}.{f_name}"), f_ty, scalar_input_values)?,
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Ty::Tuple(tys) => Ok(T::new_tuple(
            tys.iter()
                .enumerate()
                .map(|(i, t_ty)| extract(&format!("{name}.{i}"), t_ty, scalar_input_values))
                .collect::<Result<Vec<_>, _>>()?,
        )),
    }
}

/// Inverse of [extract] for the value shapes `@test` supports: break a
/// constant [Value] into the flattened per-leaf scalar entries the proof
/// pipeline uses in input maps. A scalar yields a single (`name`, value)
/// entry; an array yields one entry per element under `name.0`, `name.1`,
/// ..., recursively (a `field[2][2]` named `A` yields `A.0.0` .. `A.1.1`).
/// Ordering and naming mirror [extract] and `declare_input`.
///
/// This is deliberately *value-only* and limited to arrays:
/// [Array::values] handles sparse arrays (e.g. from `[0; 4]`, whose backing
/// map is empty) by falling back to the array's default. Tuples and structs
/// are not handled — a struct lowers to a [Value::Tuple], which no longer
/// carries the field names `name.field` flattening would need; supporting
/// them will need the resolved [Ty], not just the value. `eval_test_case`
/// rejects those parameter types (and every non-scalar leaf kind) before one
/// can reach here, so the leaf arms below are exhaustive for validated
/// inputs; anything else is an internal invariant violation.
pub fn flatten(name: &str, value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::Array(arr) => arr
            .values()
            .into_iter()
            .enumerate()
            .flat_map(|(i, v)| flatten(&format!("{name}.{i}"), &v))
            .collect(),
        // The scalar leaf kinds a validated @test input can hold — the same
        // set interp::extract accepts.
        Value::Field(_) | Value::BitVector(_) | Value::Bool(_) | Value::Int(_) => {
            vec![(name.to_string(), value.clone())]
        }
        other => unreachable!(
            "non-scalar leaf {:?} in @test input {}; \
             eval_test_inputs rejects non-scalar parameter types",
            other, name
        ),
    }
}
