//! Corpus-cleanliness guard: zero bare `: Scalar` type annotations and bare
//! `-> Scalar` return codomains (tasks δ + δ-completion).
//!
//! Signal: `: *Scalar([^<a-zA-Z]|$)` (annotation) or `-> Scalar([^<a-zA-Z]|$)`
//! (codomain), with pure-comment lines and `::Scalar` excluded.
//!
//! Walks:
//!   * `examples/**/*.ri`          — design example files
//!   * `crates/**/*.ri`            — standalone fixture .ri files
//!   * `crates/**/*.rs`            — inline .ri fixtures in Rust sources
//!     (comment/doc-prose lines are excluded — see predicate)
//!   * `gui/src-tauri/**/*.rs`     — GUI inline DSL test sources
//!   * `gui/test/**/*.ri`          — GUI fixture files
//!
//! Excluded from scan (parse-only, pin literal "Scalar", never type-resolve):
//!   * `crates/reify-syntax/tests/`
//!   * `crates/reify-ast/tests/`
//!
//! Excluded from scan (conformance corpus, must-reject fixtures are chartered):
//!   * `crates/reify-spec-conformance/fixtures/` — see the exclusion arm below.
//!
//! This test is GREEN (δ migration complete). It becomes compiler-redundant
//! once γ adds `E_BARE_SCALAR`, but protects the δ→γ window as a regression
//! guard.
//!
//! A second guard over the same walk plus two named prd-gate fixtures,
//! `corpus_has_no_bare_fea_load_magnitude` (dimension-checked-readers γ2,
//! #6941), finds every stdlib FEA load ctor call whose magnitude field
//! (`PointLoad.force`, `PressureLoad.magnitude`, `TractionLoad.traction`,
//! `BodyForce.force_density`) is a bare numeric literal.
//!
//! The predicate (what a bare `Scalar` is, and what it deliberately is not)
//! is `reify_test_support::bare_scalar::line_has_bare_scalar`.

use std::path::{Path, PathBuf};

use reify_test_support::bare_scalar::{line_has_bare_scalar, strip_trailing_line_comment};

/// The Ring-1 language-spec conformance fixture tree, repo-relative — excluded
/// from the scan, and the home of the probe that keeps that exclusion honest.
const SPEC_CONFORMANCE_FIXTURES: &str = "crates/reify-spec-conformance/fixtures";

/// The running checkout's workspace root, canonicalized.
fn workspace_root() -> PathBuf {
    reify_test_support::workspace_root::workspace_root()
        .canonicalize()
        .expect("workspace root must be accessible")
}

/// Walk `dir` recursively, appending every file whose extension equals `ext`
/// to `out`.  Silently skips unreadable entries.
fn collect_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

/// The scanned corpus both guards in this file read: the walk and the
/// exclusions listed in the module header.
fn corpus_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();

    // A. examples/**/*.ri — design example files
    collect_files(&root.join("examples"), "ri", &mut files);

    // B + C + D. crates/**/*.ri (fixture .ri files) + crates/**/*.rs (inline fixtures, doc-prose)
    collect_files(&root.join("crates"), "ri", &mut files);
    collect_files(&root.join("crates"), "rs", &mut files);

    // E. gui/src-tauri/**/*.rs — all GUI src-tauri Rust sources (production + inline-DSL
    //    tests). The recursive walk intentionally covers production files (engine.rs,
    //    types.rs, …) as well as the inline-DSL test fixtures; the wider coverage is
    //    beneficial — any bare `Scalar` reaching type resolution from GUI code is caught
    //    here too. (δ-completion: δ's guard never scanned this tree.)
    collect_files(&root.join("gui").join("src-tauri"), "rs", &mut files);

    // F. GUI fixture .ri files
    let gui_fixtures = root.join("gui").join("test");
    collect_files(&gui_fixtures, "ri", &mut files);

    // Deduplicate: the crates/ walk can't overlap with examples/ or gui/, but
    // sort + dedup keeps the list tidy.
    files.sort();
    files.dedup();

    // Exclude this guard-test file itself — it contains `: Scalar` and `-> Scalar`
    // in its own comments, strings, and unit-test literals.  Scanning it would create
    // self-referential false positives that prevent the test from ever going GREEN.
    files.retain(|p| p.file_name().and_then(|f| f.to_str()) != Some("corpus_no_bare_scalar.rs"));

    // Exclude the predicate's own module for the same reason: its unit tests
    // are bare-`Scalar` literals by construction.
    let predicate_module = root.join("crates/reify-test-support/src/bare_scalar.rs");
    files.retain(|p| p != &predicate_module);

    // Exclude parse-only test directories — they pin the LITERAL PARSED name
    // "Scalar" (never reach type resolution, can never be E_BARE_SCALAR violators).
    //   * crates/reify-syntax/tests/ — field_tests.rs:30,64 assert codomain_type.to_string()=="Scalar"
    //   * crates/reify-ast/tests/   — api_surface.rs:70 asserts name=="Scalar"
    //
    // Accepted blind spot: the exclusion is directory-level, not file-level.
    // A new test file added to either directory would also be excluded from the
    // scan.  This is intentional: every test in reify-syntax/tests/ and
    // reify-ast/tests/ is parse-only by design — none reach type resolution,
    // so none can ever be E_BARE_SCALAR (γ) violators.  The carve-out matches
    // the invariant the directories enforce, not just the two current files.
    let syntax_tests = root.join("crates").join("reify-syntax").join("tests");
    let ast_tests = root.join("crates").join("reify-ast").join("tests");
    files.retain(|p| !p.starts_with(&syntax_tests) && !p.starts_with(&ast_tests));

    // The Ring-1 conformance fixture tree holds CHARTERED must-reject fixtures
    // (PRD `docs/prds/v0_6/spec-conformance-suite.md` D2) — bare-`Scalar`
    // rejection is ITSELF a spec clause the suite must be free to test with a
    // violating fixture. Directory-level for the same reason as the arm above.
    // Charter: that tree's `README.md`. The test below keeps this arm
    // non-vacuous.
    let spec_conformance_fixtures = root.join(SPEC_CONFORMANCE_FIXTURES);
    files.retain(|p| !p.starts_with(&spec_conformance_fixtures));
    files
}

#[test]
fn corpus_has_zero_bare_scalar() {
    let root = workspace_root();
    let files = corpus_files(&root);

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = path.strip_prefix(&root).unwrap_or(path);
        for (line_idx, line) in content.lines().enumerate() {
            if line_has_bare_scalar(line) {
                violations.push(format!(
                    "{}:{}: {}",
                    rel.display(),
                    line_idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} bare `Scalar` annotation(s) or codomain(s). \
         Migrate each `: Scalar` -> `: Length` and `-> Scalar` -> `-> Length`:\n\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// The spec-conformance exclusion arm above is non-vacuous only while a live
/// violator sits under it: the committed placement probe. If this guard is
/// retired as compiler-redundant (see the header), retire the probe with it.
#[test]
fn spec_conformance_placement_probe_is_a_live_violator() {
    let probe = workspace_root()
        .join(SPEC_CONFORMANCE_FIXTURES)
        .join("_placement-probe/placement_probe.ri");

    let content = std::fs::read_to_string(&probe)
        .unwrap_or_else(|e| panic!("placement probe {} unreadable: {e}", probe.display()));

    assert!(
        content.lines().any(line_has_bare_scalar),
        "The placement probe {} no longer trips this guard's predicate, so the \
         spec-conformance exclusion arm is vacuous. Do NOT \"fix\" the probe: \
         restore its bare annotation, or retire the arm and the probe together. \
         See {SPEC_CONFORMANCE_FIXTURES}/README.md.",
        probe.display()
    );
}

// ── γ2 (#6941): no bare FEA load magnitude ─────────────────────────────────

/// The prd-gate fixtures the load-magnitude guard reads on top of
/// [`corpus_files`]: the one holding the deliberate bare before-images, and its
/// dimensioned solver twin.
///
/// Named leaf by leaf, never by walking `tests/prd-gate/fixtures/`. verify.sh's
/// no-heavy-checks carve-out for that directory rests on "nothing globs it"
/// (`tests/infra/test_verify_scope.sh`, PG-DRIFT-DIR), and both leaves are in
/// verify.sh's `_RUST_COUPLED_RI_FIXTURES`. Every other fixture spelling a bare
/// magnitude is a compile-time ctor-conformance Error since γ2.
const PRD_GATE_LOAD_FIXTURES: [&str; 2] = [
    "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
    "tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri",
];

/// Each stdlib FEA load ctor and its magnitude field, typed `Force`,
/// `Pressure`, `Pressure` and `ForceDensity` in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` since γ2 (#6941).
const FEA_LOAD_MAGNITUDE_FIELDS: [(&str, &str); 4] = [
    ("PointLoad", "force"),
    ("PressureLoad", "magnitude"),
    ("TractionLoad", "traction"),
    ("BodyForce", "force_density"),
];

/// The deliberate bare spellings, one site per `(file, field)`, owned by #8008.
///
/// They are before-images of the silent-unit defect, wrong on purpose, and the
/// same three sites are deferred to #8008 in reify-compiler's
/// `CTOR_CONFORMANCE_CORPUS_RESIDUAL`. A reify-cli test cannot import that
/// const, so the facts are restated here and the guard reds when one of them
/// stops naming exactly one live site.
const DELIBERATE_BARE_LOAD_MAGNITUDES: [(&str, &str); 3] = [
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "force",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "magnitude",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "force_density",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MagnitudeSpelling {
    /// A numeric literal alone: `1000.0`, `-77000.0`, `1e6`.
    Bare,
    /// A numeric literal carrying a unit: `1000N`, `5e6Pa`, `-9.81N/m^3`.
    Dimensioned,
    /// Anything else: an identifier, a call, an arithmetic expression.
    Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadMagnitudeSite {
    field: &'static str,
    /// 1-based line of the `field:` argument.
    line: usize,
    value: String,
    spelling: MagnitudeSpelling,
}

/// Every magnitude argument passed to a stdlib FEA load ctor in `source`.
///
/// A ctor declared in the call's own scope resolves to that local shadow, so
/// its calls are not sites; comment text is never a site. Each raw string
/// literal is a scope of its own (one embedded `.ri` source), and the text
/// outside every raw string literal is one more.
fn load_magnitude_sites(source: &str) -> Vec<LoadMagnitudeSite> {
    let code = without_line_comments(source);
    let bodies = raw_string_bodies(&code);
    let mut outside = code.clone().into_bytes();
    for body in &bodies {
        outside[body.clone()].fill(b' ');
    }
    let outside = String::from_utf8_lossy(&outside);
    let mut sites = Vec::new();
    for (ctor, field) in FEA_LOAD_MAGNITUDE_FIELDS {
        let shadowed = |at: usize| match bodies.iter().find(|body| body.contains(&at)) {
            Some(body) => declares_structure(&code[body.clone()], ctor),
            None => declares_structure(&outside, ctor),
        };
        for open in call_opens(&code, ctor) {
            if shadowed(open) {
                continue;
            }
            for (offset, argument) in call_arguments(&code, open) {
                let Some((name, value)) = argument.split_once(':') else {
                    continue;
                };
                if name.trim() != field {
                    continue;
                }
                let name_at = offset + (argument.len() - argument.trim_start().len());
                let value = value.trim();
                sites.push(LoadMagnitudeSite {
                    field,
                    line: 1 + code[..name_at].matches('\n').count(),
                    value: value.to_owned(),
                    spelling: spelling_of(value),
                });
            }
        }
    }
    sites
}

/// The byte range of every raw string literal's body (`r"…"`, `r#"…"#`, …)
/// in `code`.
fn raw_string_bodies(code: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = code.as_bytes();
    let mut bodies = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let opens_literal = i == 0 || !(is_ident_byte(bytes[i - 1]) || bytes[i - 1] == b'"');
        if bytes[i] == b'r' && opens_literal {
            let quote = i + 1 + bytes[i + 1..].iter().take_while(|&&b| b == b'#').count();
            if bytes.get(quote) == Some(&b'"') {
                let closer = format!("\"{}", "#".repeat(quote - i - 1));
                let start = quote + 1;
                if let Some(len) = code[start..].find(&closer) {
                    bodies.push(start..start + len);
                    i = start + len + closer.len();
                    continue;
                }
            }
        }
        i += 1;
    }
    bodies
}

/// `source` with every pure comment line blanked and every trailing line
/// comment stripped; line numbers are preserved.
fn without_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                strip_trailing_line_comment(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `code` declares a structure named `name` (`structure def Name` or
/// `structure Name`).
fn declares_structure(code: &str, name: &str) -> bool {
    ["structure def ", "structure "].iter().any(|keyword| {
        let needle = format!("{keyword}{name}");
        code.match_indices(&needle).any(|(at, _)| {
            let end = at + needle.len();
            !code.as_bytes().get(end).is_some_and(|&b| is_ident_byte(b))
        })
    })
}

/// Byte offset of the `(` of every `ctor(` call in `code`; a longer
/// identifier ending in `ctor` is not a call of it.
fn call_opens(code: &str, ctor: &str) -> Vec<usize> {
    let needle = format!("{ctor}(");
    code.match_indices(&needle)
        .filter(|&(at, _)| at == 0 || !is_ident_byte(code.as_bytes()[at - 1]))
        .map(|(at, _)| at + ctor.len())
        .collect()
}

/// The top-level arguments of the call whose `(` is at `open`, each with its
/// byte offset; empty when the parentheses never balance.
fn call_arguments(code: &str, open: usize) -> Vec<(usize, &str)> {
    let mut arguments = Vec::new();
    let mut start = open + 1;
    let mut depth = 0usize;
    for (i, byte) in code.bytes().enumerate().skip(open + 1) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b')' => {
                arguments.push((start, &code[start..i]));
                return arguments;
            }
            b']' | b'}' => return Vec::new(),
            b',' if depth == 0 => {
                arguments.push((start, &code[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    Vec::new()
}

fn spelling_of(value: &str) -> MagnitudeSpelling {
    match numeric_literal_len(value) {
        Some(len) if len == value.len() => MagnitudeSpelling::Bare,
        Some(len) if value[len..].starts_with(|c: char| c.is_ascii_alphabetic()) => {
            MagnitudeSpelling::Dimensioned
        }
        _ => MagnitudeSpelling::Expression,
    }
}

/// Byte length of the numeric literal `value` starts with (optional sign,
/// digits with an optional fraction, optional exponent), or `None`.
fn numeric_literal_len(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let digits_from = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let sign = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_end = digits_from(sign);
    let mut end = integer_end;
    if bytes.get(end) == Some(&b'.') {
        end = digits_from(end + 1);
    }
    if integer_end == sign && end <= sign + 1 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent = end + 1 + usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            end = digits_from(exponent);
        }
    }
    Some(end)
}

#[test]
fn corpus_has_no_bare_fea_load_magnitude() {
    let root = workspace_root();
    let mut files = corpus_files(&root);
    files.extend(PRD_GATE_LOAD_FIXTURES.iter().map(|rel| root.join(rel)));

    let mut bare: Vec<String> = Vec::new();
    let mut deliberate_hits = [0usize; DELIBERATE_BARE_LOAD_MAGNITUDES.len()];
    let mut dimensioned_fields = std::collections::BTreeSet::new();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy();
        for site in load_magnitude_sites(&content) {
            let deliberate = DELIBERATE_BARE_LOAD_MAGNITUDES
                .iter()
                .position(|&(file, field)| file == rel && field == site.field);
            match (site.spelling, deliberate) {
                (MagnitudeSpelling::Dimensioned, _) => {
                    dimensioned_fields.insert(site.field);
                }
                (MagnitudeSpelling::Bare, Some(index)) => deliberate_hits[index] += 1,
                (MagnitudeSpelling::Bare, None) => {
                    bare.push(format!(
                        "{rel}:{}: {}: {}",
                        site.line, site.field, site.value
                    ));
                }
                (MagnitudeSpelling::Expression, _) => {}
            }
        }
    }

    assert!(
        bare.is_empty(),
        "Found {} bare FEA load magnitude(s). The fields are dimensioned since γ2 \
         (#6941), so a bare literal is a compile Error: spell force in N, \
         magnitude and traction in Pa, force_density in N/m^3.\n\n{}",
        bare.len(),
        bare.join("\n")
    );

    let stale: Vec<String> = DELIBERATE_BARE_LOAD_MAGNITUDES
        .iter()
        .zip(deliberate_hits)
        .filter(|&(_, hits)| hits != 1)
        .map(|((file, field), hits)| format!("{file} :: {field} ({hits} bare site(s))"))
        .collect();
    assert!(
        stale.is_empty(),
        "each DELIBERATE_BARE_LOAD_MAGNITUDES entry must name exactly one live bare \
         site; retire the entry with its site (owner #8008):\n{}",
        stale.join("\n")
    );

    let never_dimensioned: Vec<&str> = FEA_LOAD_MAGNITUDE_FIELDS
        .iter()
        .map(|&(_, field)| field)
        .filter(|field| !dimensioned_fields.contains(field))
        .collect();
    assert!(
        never_dimensioned.is_empty(),
        "anti-vacuity: the scan saw no unit-carrying site at {never_dimensioned:?}, so \
         it cannot be telling a dimensioned spelling from a bare one there"
    );
}

#[cfg(test)]
mod load_site_predicate_tests {
    use super::{MagnitudeSpelling, load_magnitude_sites};

    fn sites(source: &str) -> Vec<(&'static str, usize, MagnitudeSpelling)> {
        load_magnitude_sites(source)
            .into_iter()
            .map(|site| (site.field, site.line, site.spelling))
            .collect()
    }

    #[test]
    fn multi_line_call_reports_the_line_of_the_field() {
        let source = "let p = PointLoad(\n    point: \"tip\",\n    force: 1000.0,\n)";
        assert_eq!(sites(source), [("force", 3, MagnitudeSpelling::Bare)]);
    }

    #[test]
    fn negative_literal_is_bare() {
        let source = r#"let b = BodyForce(body: "all", force_density: -77000.0)"#;
        assert_eq!(
            sites(source),
            [("force_density", 1, MagnitudeSpelling::Bare)]
        );
    }

    #[test]
    fn exponent_literal_is_bare() {
        assert_eq!(
            sites("let p = PressureLoad(magnitude: 1e6)"),
            [("magnitude", 1, MagnitudeSpelling::Bare)]
        );
    }

    #[test]
    fn unit_suffixed_literal_is_dimensioned() {
        let source = "PointLoad(force: 1000N)\nTractionLoad(traction: 5e6Pa)\n\
                      BodyForce(force_density: -77000N/m^3)";
        assert_eq!(
            sites(source),
            [
                ("force", 1, MagnitudeSpelling::Dimensioned),
                ("traction", 2, MagnitudeSpelling::Dimensioned),
                ("force_density", 3, MagnitudeSpelling::Dimensioned),
            ]
        );
    }

    #[test]
    fn expression_is_neither_bare_nor_dimensioned() {
        assert_eq!(
            sites("PointLoad(force: tip_force * 2.0)"),
            [("force", 1, MagnitudeSpelling::Expression)]
        );
    }

    #[test]
    fn commented_out_call_is_not_a_site() {
        let source = "// let p = PointLoad(force: 1000.0)\n\
                      let q = PointLoad() // was PointLoad(force: 1.0)";
        assert_eq!(sites(source), []);
    }

    #[test]
    fn nested_commas_and_other_ctors_fields_are_not_split_or_matched() {
        let source = "PointLoad(direction: vec3(1.0, 0.0, 0.0), force: 2.0)\n\
                      BodyForce(force: 3.0)\nMyPointLoad(force: 4.0)";
        assert_eq!(sites(source), [("force", 1, MagnitudeSpelling::Bare)]);
    }

    #[test]
    fn a_locally_declared_ctor_is_a_shadow_not_a_site() {
        let source = "structure def PressureLoad { param magnitude : Real }\n\
                      let p = PressureLoad(magnitude: 1.0)\n\
                      let q = PointLoad(force: 1.0)";
        assert_eq!(sites(source), [("force", 3, MagnitudeSpelling::Bare)]);
    }

    #[test]
    fn a_shadow_covers_only_the_raw_string_that_declares_it() {
        let source = "const SHADOWED: &str = r#\"\n\
                      structure def PressureLoad { param magnitude : Real }\n\
                      let p = PressureLoad(magnitude: 1.0)\n\
                      \"#;\n\
                      const STDLIB: &str = r#\"\n\
                      let q = PressureLoad(magnitude: 2.0)\n\
                      \"#;";
        assert_eq!(sites(source), [("magnitude", 6, MagnitudeSpelling::Bare)]);
    }
}
