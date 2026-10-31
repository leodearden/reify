//! Integration-test harness for the declared surface of shipped std modules: each
//! member loads one of them (`std.ports` and its mechanical/electrical/thermal/fluid
//! submodules, `std.io`, `std.stock`, the standard joint library) and pins what it
//! declares.
//!
//! Split out of `harness_modules_ports` by task #9503, when that unit crossed the
//! advisory warn line of `tests/infra/test_harness_kloc_cap.sh`. The layout contract
//! the `#[path]` lines below obey is that guard's C1/C2 header; it is kept there, not
//! restated here.
//!
//! This root declares no `common`, because no member uses `tests/common/`. That include
//! is compiled only into `harness_modules_ports`, so a std-module test that needs
//! `common` (as `standard_gravity_tests` does) belongs in `harness_modules_ports`;
//! declaring `common` in this root as well would compile a second copy of it.
#[path = "harness_modules_ports_stdlib/io_traits_tests.rs"]
mod io_traits_tests;
#[path = "harness_modules_ports_stdlib/ports_prelude_test.rs"]
mod ports_prelude_test;
#[path = "harness_modules_ports_stdlib/ports_stdlib_compile.rs"]
mod ports_stdlib_compile;
#[path = "harness_modules_ports_stdlib/standard_joint_library_tests.rs"]
mod standard_joint_library_tests;
#[path = "harness_modules_ports_stdlib/standard_stock_tests.rs"]
mod standard_stock_tests;
