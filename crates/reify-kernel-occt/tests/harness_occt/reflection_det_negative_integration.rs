//! A-ε probe (task #6619, PRD `docs/prds/v0_6/assembly-derivation-toolbox.md`
//! leaf A-ε, boundary test T17): placeholder header. Replaced with the full
//! finding write-up in the doc-only step that lands last in this module's
//! plan.

#![cfg(has_occt)]

use reify_ir::{ExportFormat, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};
