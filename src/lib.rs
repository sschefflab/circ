//! # CirC
//!
//! A compiler infrastructure for compiling programs to circuits

#![warn(missing_docs)]
// #![deny(warnings)]
#![allow(rustdoc::private_intra_doc_links)]
#![allow(clippy::mutable_key_type)]

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[macro_use]
pub mod ir;
pub mod cfg;
pub mod circify;
pub mod compile;
pub mod create_input;
pub mod front;
#[cfg(feature = "spartan")]
pub mod right_field_arithmetic;
pub mod target;
pub mod util;
// Orchestrates the ZoKratesCurly @test pipeline (frontend -> compile ->
// proof backend), so it needs the frontend's features plus R1CS. It sits
// above `front` on purpose: the frontend produces validated test metadata,
// this module consumes it — never the other way around.
#[cfg(all(feature = "smt", feature = "zokc", feature = "r1cs"))]
pub mod test_runner;
