//! Set lookup arguments
use super::hash::UniversalHasher;
use super::*;
use crate::util::ns::Namespace;
use log::debug;

use std::convert::TryInto;

/// Extract the length and field type from a composite sort (Tuple or Array of fields).
/// Returns None for scalar field elements.
fn composite_sort_info(sort: &Sort) -> Option<(usize, circ_fields::FieldT)> {
    match sort {
        Sort::Tuple(tuple_sorts) => {
            let len = tuple_sorts.len();
            let f = tuple_sorts[0].as_pf().clone();
            Some((len, f))
        }
        Sort::Array(array_sort) => {
            let len = array_sort.size;
            let f = array_sort.val.as_pf().clone();
            Some((len, f))
        }
        _ => None,
    }
}

/// Extract elements from a term that is either a Tuple or an Array.
fn extract_elements(t: Term) -> Vec<Term> {
    let sort = check(&t);
    match sort {
        Sort::Tuple(_) => tuple_terms(t),
        Sort::Array(_) => extras::array_elements(&t),
        _ => panic!("extract_elements called on non-composite sort"),
    }
}

/// Do set lookup arguments
pub fn apply(c: &mut Computation) {
    let mut asserted_map_contains_keys = TermSet::default();
    assert_eq!(c.outputs.len(), 1);
    extras::collect_asserted_ops(
        &c.outputs[0],
        &|o: &Op| o == &Op::ExtOp(ExtOp::MapContainsKey),
        &mut asserted_map_contains_keys,
    );
    if asserted_map_contains_keys.is_empty() {
        return;
    }
    let mut maps_to_keys: TermMap<Vec<Term>> = TermMap::default();
    for containment in &asserted_map_contains_keys {
        let [map, key]: &[Term; 2] = containment.cs().try_into().unwrap();
        maps_to_keys
            .entry(map.clone())
            .or_default()
            .push(key.clone());
    }
    let ns = Namespace::new();
    let mut to_assert = Vec::new();
    for (i, (map, keys)) in maps_to_keys.into_iter().enumerate() {
        assert!(
            map.is_const(),
            "set membership only supported for constant sets"
        );
        debug!(
            "set membership argument; set size {}, key count {}",
            map.as_map_opt().unwrap().map.len(),
            keys.len()
        );
        let haystack: Vec<Term> = map
            .as_map_opt()
            .unwrap()
            .map
            .keys()
            .cloned()
            .map(const_)
            .collect();

        // Check if keys are composite (tuples or arrays) or scalar fields
        let key_sort = check(&keys[0]);
        if let Some((elem_count, f)) = composite_sort_info(&key_sort) {
            // Composite path: hash to field elements first
            // Flatten composites to Vec<Vec<Term>>
            let haystack_vecs: Vec<Vec<Term>> =
                haystack.into_iter().map(extract_elements).collect();
            let needle_vecs: Vec<Vec<Term>> =
                keys.into_iter().map(extract_elements).collect();

            // Collect all inputs for challenge derivation
            let inputs: Vec<Term> = haystack_vecs
                .iter()
                .chain(&needle_vecs)
                .flatten()
                .cloned()
                .collect();

            // Hash each composite to a single field element
            let uhf = UniversalHasher::new(
                ns.fqn(format!("setmem{}_uhf", i)),
                &f,
                inputs.clone(),
                elem_count,
            );
            let hashed_haystack: Vec<Term> =
                haystack_vecs.into_iter().map(|v| uhf.hash(v)).collect();
            let hashed_needles: Vec<Term> =
                needle_vecs.into_iter().map(|v| uhf.hash(v)).collect();

            to_assert.push(super::checker::rom::lookup(
                c,
                ns.subspace(format!("setmem{}", i)),
                hashed_haystack,
                hashed_needles,
                Some(inputs),
            ));
        } else {
            // Original path: scalar field elements
            to_assert.push(super::checker::rom::lookup(
                c,
                ns.subspace(format!("setmem{}", i)),
                haystack,
                keys,
                None,
            ));
        }
    }
    to_assert.push(c.outputs[0].clone());
    let subs: TermMap<Term> = asserted_map_contains_keys
        .into_iter()
        .map(|c| (c, bool_lit(true)))
        .collect();
    c.outputs = vec![extras::substitute(&term(AND, to_assert), subs)];
}
