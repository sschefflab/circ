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

/// Converts a scalar or array test input into the named scalar values expected
/// by the proof pipeline.
///
/// This reverses [`extract`]. Scalars keep their names, while array elements
/// use dotted indices. For example, a `field[2][2]` named `A` becomes
/// `A.0.0`, `A.0.1`, `A.1.0`, and `A.1.1`.
///
/// [`Array::values`] expands repeated arrays such as `[0; 4]`.
///
/// Tuples and structs are not supported. `eval_test_case` rejects them before
/// the inputs reach this function.
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
