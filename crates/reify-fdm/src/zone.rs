// SPDX-License-Identifier: AGPL-3.0-or-later

//! R-fast geometric zone classifier (task γ).
//!
//! Implements the wall / skin / infill trichotomy from
//! `docs/prds/v0_5/fdm-as-printed-fea.md` §C4 as a pure function over
//! precomputed distance probes. The classifier is consumer-agnostic —
//! the δ-task is responsible for wiring real-body OCCT distance queries
//! into `ZoneProbe` values; this module only knows how to interpret them.
