//! Hand-rolled `unsafe extern "C"` FFI bindings to libgmsh 4.15.2.
//!
//! Populated incrementally — see task 3092 plan steps 2 and 4. This file
//! exists with module declaration in `lib.rs` so the cfg-gated module tree
//! is in place before the FFI bindings land.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.
