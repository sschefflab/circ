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
pub mod compile;
pub mod cfg;
pub mod circify;
pub mod front;
pub mod target;
pub mod util;
pub mod create_input;
#[cfg(feature = "spartan")]
pub mod right_field_arithmetic;
