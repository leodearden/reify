//! The D9 owner group a surveyed site belongs to, and the advisory remedy hint
//! rendered beside it.

/// Which D9 fix-forward rule governs a site — the load-bearing, mechanizable
/// half of D9 and the artifact's primary grouping key.
///
/// This does NOT encode D9's split between class (1) call-site bug and class
/// (2) wrong declared field type. The PRD defines that as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; β must not fabricate it.
///
/// # The derives are load-bearing, not decoration
///
/// `EnumIter` + `Ord` are what make [`Owner::render_order`] DERIVED from this
/// declaration instead of restated as an array literal at the render site. The
/// variant order below therefore IS the artifact's group order, and adding a
/// variant automatically adds its group. See [`Owner::render_order`] for why
/// that matters more here than anywhere else in the survey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
pub(crate) enum Owner {
    /// The site's structure def is declared in an FEA stdlib module. Per D9,
    /// γ may make CALL-SITE changes only — field-type flips stay v0.6-owned.
    FeaDeferredToV06,
    /// The recovered name IS a declared `structure def` somewhere in the swept
    /// corpus (or the stdlib) and is not FEA-owned: D9's per-case judgment
    /// applies, and this is the only bucket γ should size as actionable.
    NonFea,
    /// A name was recovered at the anchor, but it is not a declared
    /// `structure def` anywhere.
    ///
    /// `ctor_type_name_at` recovers *any* identifier followed by `(` — it
    /// cannot tell a ctor from a plain function call. Several codes in the
    /// admission set (`SelectorKindMismatch` from selector composition and
    /// overload resolution) reach this survey from a NON-ctor path, where the
    /// anchor identifier is a FUNCTION name: `union(faces(b), edges(b))` and
    /// `needs_face(edges(b))` both appear in the tracked corpus. Letting those
    /// fall into [`Owner::NonFea`] would size a function call into γ's
    /// actionable pile — exactly the "unattributable site must never default
    /// into the touchable bucket" rule this survey states, violated one level
    /// down from where it was being enforced.
    UnresolvedDef,
    /// No def name could be attributed at all — the label span names none and
    /// the diagnostic prose names none. Each row carries its own machine-derived
    /// `DefOrigin` saying which shape it was.
    ///
    /// Deliberately its own bucket: silently defaulting an unattributable site
    /// into the touchable pile would be the one classification error with a real
    /// cost.
    Unknown,
}

impl Owner {
    /// Every owner class, in the order the artifact renders their groups.
    ///
    /// DERIVED from the enum declaration via `strum::EnumIter` and ordered by
    /// the `Ord` derive — deliberately NOT an array literal at the render site.
    /// A hand-written group list is the one drift this artifact cannot afford:
    /// a fifth `Owner` variant would compile cleanly, render no group at all,
    /// and silently drop every site classified into it — while the header still
    /// printed `**Sites:** N` counting it. "A class the renderer forgets is a
    /// class of sites that silently vanishes from the artifact" is this
    /// survey's own statement of its one unacceptable failure; a literal makes
    /// that failure reachable by omission, and a guard test that iterates its
    /// OWN copy of the same literal cannot see it either.
    ///
    /// `strum` is already a `[dev-dependencies]` entry of this crate (the ε2
    /// `TypeDiscriminants` canary), so this costs no new dependency.
    pub(crate) fn render_order() -> Vec<Owner> {
        use strum::IntoEnumIterator;
        let mut all: Vec<Owner> = Owner::iter().collect();
        // The FEA do-not-touch partition first, then the actionable non-FEA
        // group, then the two manual-triage buckets — which is exactly the
        // declaration order, pinned here through `Ord` rather than assumed
        // from `EnumIter`'s traversal.
        all.sort_unstable();
        all
    }

    /// Stable section title for the rendered artifact.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Owner::FeaDeferredToV06 => "FEA — deferred to v0.6 (DO NOT FIX HERE)",
            Owner::NonFea => "non-FEA structure def — γ per-case judgment",
            Owner::UnresolvedDef => {
                "name recovered, but it is not a known structure def — needs manual triage"
            }
            Owner::Unknown => "unattributed def — needs manual triage",
        }
    }
}

// ─── step 7/8: D9 owner classification ───────────────────────────────────────

/// Absolute path to the stdlib directory whose FEA modules define the
/// do-not-touch partition.
const STDLIB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib");

/// The FEA stdlib module FILE STEMS, as PRD §4 D9 itself enumerates them
/// ("`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …"), reconciled against the
/// live `ls crates/reify-compiler/stdlib/`.
///
/// This small MODULE list is the survey's only reviewable knob. The def NAMES
/// are always derived by scanning these files, never hand-listed — which is
/// what keeps "zero hand-derived entries" literally true, and keeps the
/// do-not-touch partition traceable to the PRD rather than to β's judgment.
///
/// [`scan_structure_defs`] panics if any stem here has no `.ri` file, so a
/// stdlib rename cannot silently empty the partition.
const FEA_STDLIB_MODULES: &[&str] = &[
    "fea",
    "fea_multi_case",
    "fea_types",
    "materials_fea",
    "solver_buckling",
    "solver_buckling_fns",
    "solver_elastic",
];

/// Stdlib modules whose NAME reads as FEA-family but which are deliberately NOT
/// in the D9 do-not-touch partition.
///
/// This list exists so that "not FEA-owned" is a RECORDED decision rather than
/// an omission. [`every_fea_family_shaped_stdlib_module_is_classified`] requires
/// every FEA-shaped stem to appear in exactly one of the two lists, so adding
/// `fea_contact.ri` (or a fourth `modal_*`) to the stdlib turns that guard red
/// instead of silently routing its defs into `Owner::NonFea` — the group the
/// artifact labels "the group to size γ against". Mis-classifying INTO that
/// group is, per [`d9_owner`], "the one classification error with a real cost",
/// and until this guard existed it was the only classification path with no
/// drift check at all.
///
/// Why `modal_*` is on THIS side of the line: PRD §4 D9 defines the deferred
/// partition by the v0.6 migration it points at
/// (`docs/prds/v0_6/fea-load-support-selector-migration.md`) — the FEA load and
/// boundary-condition defs whose String→selector field flips are v0.6-owned —
/// and enumerates it as "`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …".
/// `modal_analysis.ri` is structural dynamics, not that migration's surface: its
/// forcing-function defs already declare selector-typed fields
/// (`structure def StepForce { param at : Selector … }`, `modal_analysis.ri:490`),
/// so a ctor row against one of them is ordinary call-site work for γ, with no
/// field-type flip to defer. The two `modal_*_fns` modules declare no
/// `structure def` at all, so their placement is inert either way and is
/// recorded only to keep the shape sweep exhaustive.
const DELIBERATELY_NOT_FEA_OWNED: &[&str] = &[
    "modal_analysis",
    "modal_analysis_fns",
    "modal_mechanism_fns",
];

/// True for a stdlib module stem that reads as FEA-family.
///
/// Deliberately WIDER than [`FEA_STDLIB_MODULES`]: its job is to catch a new
/// module that a reader would plausibly expect in the deferred partition, and
/// force a classification. A name outside every shape here (say a future
/// `contact_mechanics.ri`) is not caught — no naming rule can be complete —
/// which is why the FEA list stays a reviewable knob rather than a derived one.
fn is_fea_family_shaped(stem: &str) -> bool {
    stem == "fea"
        || stem.starts_with("fea_")
        || stem.ends_with("_fea")
        || stem.starts_with("solver_")
        || stem.starts_with("modal_")
}

#[test]
fn every_fea_family_shaped_stdlib_module_is_classified() {
    let dir = std::path::Path::new(STDLIB_DIR);
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read the stdlib dir {}: {e}", dir.display()));

    let mut shaped: Vec<String> = Vec::new();
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()))
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        if is_fea_family_shaped(&stem) {
            shaped.push(stem);
        }
    }
    shaped.sort();
    assert!(
        !shaped.is_empty(),
        "the shape sweep matched NO stdlib module — the enumeration or the shape \
         predicate has broken, and this guard would be vacuously green"
    );

    let unclassified: Vec<&String> = shaped
        .iter()
        .filter(|stem| {
            !FEA_STDLIB_MODULES.contains(&stem.as_str())
                && !DELIBERATELY_NOT_FEA_OWNED.contains(&stem.as_str())
        })
        .collect();
    assert!(
        unclassified.is_empty(),
        "these stdlib modules read as FEA-family but are in neither \
         FEA_STDLIB_MODULES nor DELIBERATELY_NOT_FEA_OWNED: {unclassified:?}. \
         Leaving one off is not inert: its defs route to `Owner::NonFea`, the \
         group the artifact tells γ to size and fix. Add it to whichever list is \
         right — and say why, if it is the second."
    );

    let both: Vec<&&str> = FEA_STDLIB_MODULES
        .iter()
        .filter(|m| DELIBERATELY_NOT_FEA_OWNED.contains(m))
        .collect();
    assert!(
        both.is_empty(),
        "a module cannot be both FEA-owned and deliberately not: {both:?}"
    );

    // Same rename guard `scan_structure_defs` gives the FEA list, for the other
    // one: a stale exclusion is how a genuinely FEA-shaped NEW module can slip
    // past the check above under an old name.
    for stem in DELIBERATELY_NOT_FEA_OWNED {
        let path = dir.join(format!("{stem}.ri"));
        assert!(
            path.exists(),
            "DELIBERATELY_NOT_FEA_OWNED lists '{stem}' but {} does not exist — a \
             stdlib rename must not leave a stale exclusion behind",
            path.display()
        );
    }
}

/// The `structure def <Name>` declarations in `dir/<stem>.ri` for each `stem`.
///
/// Anchored at the START OF A LINE rather than matched as a substring,
/// deliberately: a naive scan of `stdlib/fea_multi_case.ri` harvests `already`
/// as a def name from the prose "…(its structure def already declares…" in a
/// comment at line 292. Leading whitespace is trimmed first, so an INDENTED
/// declaration counts; a `//` line still fails the keyword strip after trimming,
/// which is what keeps that guard intact.
///
/// # Panics
///
/// If a listed module has no file. That is the deliberate loud failure: a
/// silently-empty FEA partition would mis-classify every v0.6-deferred site as
/// touchable, which is the single most costly error this artifact could make.
fn scan_structure_defs(
    dir: &std::path::Path,
    modules: &[&str],
) -> std::collections::BTreeSet<String> {
    let mut defs = std::collections::BTreeSet::new();
    for stem in modules {
        let path = dir.join(format!("{stem}.ri"));
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: FEA stdlib module '{stem}' is listed in \
                 FEA_STDLIB_MODULES but {} cannot be read: {e}. A stdlib rename must not \
                 silently empty the D9 do-not-touch partition — update the list.",
                path.display()
            )
        });
        collect_structure_defs_into(&source, &mut defs);
    }
    defs
}

/// Add every `structure def <Name>` declared by `source` to `defs`.
///
/// The line-start anchor and the `pub `/`priv ` visibility prefixes are the whole
/// grammar. Leading whitespace is trimmed before the strip, because this scanner
/// serves BOTH corpus halves and they are indented differently: all 719
/// declarations in the tracked `.ri` corpus sit at column 0, so the trim is a
/// measured no-op there (that corpus has zero indented declarations), but Reify
/// embedded in a Rust raw-string literal is routinely indented to match the
/// surrounding Rust — 480 of the 2,228 declarations across `crates/**/*.rs` carry
/// leading whitespace. Anchoring at column 0 hid every one of those from the
/// known-def set and demoted each site constructing them to
/// [`Owner::UnresolvedDef`], a factually false owner attribution in an artifact
/// whose Provenance section promises machine-derived rows.
///
/// Missing the visibility prefix would likewise drop `pub structure def Actuator`
/// from the known set and demote its sites — conservative, but needless noise in
/// γ's triage.
pub(crate) fn collect_structure_defs_into(source: &str, defs: &mut std::collections::BTreeSet<String>) {
    const DEF_KEYWORD: &str = "structure def ";
    const VISIBILITY_PREFIXES: &[&str] = &["pub ", "priv "];
    for line in source.lines() {
        // Line-start anchor, after trimming indentation: a declaration may be
        // INDENTED (the inline-fixture shape), but a comment or a mid-line
        // mention still fails the keyword strip. A naive SUBSTRING scan of
        // `stdlib/fea_multi_case.ri` harvests `already` from the comment
        // "…(its structure def already declares…"; that line still opens with
        // `// ` after the trim, so it still fails `strip_prefix(DEF_KEYWORD)`.
        let line = line.trim_start();
        let after_vis = VISIBILITY_PREFIXES
            .iter()
            .find_map(|p| line.strip_prefix(p))
            .unwrap_or(line);
        let Some(rest) = after_vis.strip_prefix(DEF_KEYWORD) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            defs.insert(name);
        }
    }
}

/// Every `structure def` declared anywhere in `crates/reify-compiler/stdlib/`.
///
/// Seeds the known-def set so a site constructing a stdlib def still resolves
/// even when the declaring stdlib file was not itself part of the swept corpus.
///
/// # Panics
///
/// On ANY I/O failure — an unreadable directory entry, or a `*.ri` under
/// [`STDLIB_DIR`] that cannot be read. Same loud-failure contract as
/// [`scan_structure_defs`], and for the same reason: a silently-shrunk known-def
/// set does not fail visibly, it DEMOTES real ctor sites to
/// [`Owner::UnresolvedDef`] with no signal anywhere in the artifact. An earlier
/// draft swallowed both failures (`entries.flatten()` and an `if let Ok(…)`),
/// which is the same silent-shrink class the sibling scanner already panics on.
///
/// Scanned ONCE per process, behind the same `OnceLock` its FEA sibling
/// [`fea_owned_defs`] uses: the stdlib does not change while the test binary
/// runs, and every gate-resident test that reaches `survey_corpus` would
/// otherwise re-`read_dir` and re-read all ~46 modules from disk.
pub(crate) fn stdlib_structure_defs() -> &'static std::collections::BTreeSet<String> {
    static DEFS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    DEFS.get_or_init(scan_stdlib_structure_defs)
}

/// The uncached scan behind [`stdlib_structure_defs`].
fn scan_stdlib_structure_defs() -> std::collections::BTreeSet<String> {
    let mut defs = std::collections::BTreeSet::new();
    let dir = std::path::Path::new(STDLIB_DIR);
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "ctor_conformance_corpus_survey: cannot read the stdlib dir {}: {e}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot read an entry of the stdlib dir \
                 {}: {e}. A dropped entry would silently shrink the known-def set and \
                 demote real ctor sites to `UnresolvedDef`.",
                dir.display()
            )
        });
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot read the stdlib module {}: {e}. \
                 An unreadable stdlib file would silently drop its defs from the \
                 known-def set and demote every site constructing one of them to \
                 `UnresolvedDef`.",
                path.display()
            )
        });
        collect_structure_defs_into(&source, &mut defs);
    }
    defs
}

/// Every structure def declared by an FEA stdlib module, scanned once.
pub(crate) fn fea_owned_defs() -> &'static std::collections::BTreeSet<String> {
    static DEFS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    DEFS.get_or_init(|| scan_structure_defs(std::path::Path::new(STDLIB_DIR), FEA_STDLIB_MODULES))
}

/// The D9 fix-forward class governing a site whose structure def is `def`.
///
/// Mechanizes exactly the half of D9 that IS decidable — whether the def is
/// FEA-owned, hence call-site-changes-only with field-type flips deferred to
/// v0.6.
///
/// `Owner::NonFea` — the only bucket γ should size as actionable — is reached
/// ONLY when `def` is a name that `structure_defs` actually declares. Everything
/// else routes to a triage bucket: an unrecovered name to [`Owner::Unknown`], a
/// recovered name that is not a declared structure def to
/// [`Owner::UnresolvedDef`]. Both directions of that guard matter, because
/// `ctor_type_name_at` recovers any identifier followed by `(` and therefore
/// cannot tell a ctor from a function call; guessing in the touchable direction
/// is the one classification error with a real cost.
///
/// # Approximation: one global namespace, not per-file module scope
///
/// `structure_defs` is accumulated across the WHOLE corpus plus the stdlib, with
/// no module scoping, so `Owner::NonFea` means "some file declares this name" —
/// not "the file this site sits in can see that declaration". That over-includes
/// in the touchable direction: a site constructing `Widget` is called actionable
/// whenever ANY unrelated corpus member declares `structure def Widget`. The FEA
/// direction has the same shape but fails conservatively (over-deferring costs γ
/// sizing accuracy, never a wrong edit), so only the `NonFea` direction is
/// exposed. Removing it needs each member's import graph resolved — more
/// machinery than a one-shot snapshot warrants — so the renderer STATES the
/// approximation in the artifact's named limitations, and the `non-FEA` group
/// blurb points γ at it, rather than leaving it for a reader to infer.
pub(crate) fn d9_owner(
    def: Option<&str>,
    fea_defs: &std::collections::BTreeSet<String>,
    structure_defs: &std::collections::BTreeSet<String>,
) -> Owner {
    match def {
        None => Owner::Unknown,
        Some(name) if fea_defs.contains(name) => Owner::FeaDeferredToV06,
        Some(name) if structure_defs.contains(name) => Owner::NonFea,
        Some(_) => Owner::UnresolvedDef,
    }
}

/// The neutral hint used when no (expected, found) pattern is recognised.
const NO_HINT: &str = "no mechanical hint — γ per-case judgment";

/// Every string `reify_core::Type` renders for a selector-typed field.
///
/// Read off the Display impls, NOT guessed: `SelectorKind`'s four arms render
/// `<Kind>Selector` (`crates/reify-core/src/ty.rs`), and `Type::AnySelector`
/// renders the bare `Selector`. There is no `Selector(Face)` form anywhere —
/// an earlier draft of this file matched exactly that, and so silently gave
/// NO_HINT to every real selector site including the D3 String→selector case
/// that is the PRD's headline illegality.
const SELECTOR_TYPE_RENDERINGS: &[&str] = &[
    "Selector",
    "FaceSelector",
    "EdgeSelector",
    "VertexSelector",
    "BodySelector",
];

/// Whether `ty` renders as a selector-typed field.
fn is_selector_type(ty: &str) -> bool {
    SELECTOR_TYPE_RENDERINGS.contains(&ty)
}

/// The `Type` Display prefixes that introduce a coordinate pose.
///
/// Each is ALWAYS followed by the dimension digits in the real Display impl,
/// and then by NOTHING or by `<` — `Frame3`, `Transform3`, `Point3<Length>`
/// (`crates/reify-core/src/ty.rs`, the three `write!` arms). Those three shapes
/// are the entire pose surface; [`is_pose_type`] admits exactly them.
const POSE_TYPE_PREFIXES: &[&str] = &["Frame", "Transform", "Point"];

/// Whether `ty` renders as a coordinate pose rather than a region target.
///
/// The dimension is REQUIRED, and so is what comes after it — the remainder
/// past the prefix must be a dimension and NOTHING ELSE.
///
/// Why the predicate is this tight. `Type::StructureRef(name)` Displays as the
/// bare struct name, so any struct whose name merely STARTS like a pose reaches
/// this function as a candidate:
///
/// * A bare-prefix match would call the real defs `PointLoad` and `PointCloud`
///   poses — and `PointLoad` is the one FEA def PRD §4 D9 singles out by name.
/// * A digit-only guard (`rest` starts with an ASCII digit) is not enough
///   either: `Point3D`, `Point2Ref` and `Frame4Bar` are all perfectly legal
///   struct names carrying a digit right after the prefix. None exists in the
///   corpus today, so that was latent rather than live — but a def named
///   `Point3D` landing later would silently start collecting a pose verdict.
///
/// Either miss puts a confidently WRONG remedy string ("a pose locates a datum,
/// it does not name a region target") on rows inside the do-not-touch
/// partition, which is worse for γ's sizing than the neutral fallback. Both
/// boundaries are pinned in
/// [`selector_type_renderings_match_what_reify_core_actually_displays`].
fn is_pose_type(ty: &str) -> bool {
    POSE_TYPE_PREFIXES.iter().any(|p| {
        ty.strip_prefix(p).is_some_and(|rest| {
            let after_dim = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            // At least one digit consumed, and what follows is either the end of
            // the string (`Frame3`) or the quantity parameter (`Point3<Length>`).
            after_dim.len() < rest.len() && (after_dim.is_empty() || after_dim.starts_with('<'))
        })
    })
}

/// Whether `ty` renders as a DIMENSIONED scalar (`Scalar[…]`, not bare `Real`).
fn is_dimensioned_scalar(ty: &str) -> bool {
    ty.starts_with("Scalar[")
}

/// An ADVISORY remedy hint, derived purely and deterministically from the
/// (expected, found) type pair.
///
/// This is NOT a D9 ruling. The PRD defines the split between class (1) call-
/// site bug and class (2) wrong declared field type as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; fabricating a verdict here
/// would be exactly the hand-derivation β is forbidden. Every string below
/// therefore describes what the *shape* of the mismatch suggests, and the
/// artifact's column header says "advisory".
///
/// An unrecognised pair — or one with a missing half — gets [`NO_HINT`], never
/// an invented remedy.
pub(crate) fn remedy_hint(expected: Option<&str>, found: Option<&str>) -> String {
    let (Some(expected), Some(found)) = (expected, found) else {
        return NO_HINT.to_owned();
    };
    if is_selector_type(expected) && found == "String" {
        // D3: implicit String → selector-typed field is newly ILLEGAL; callers
        // move to typed ctors.
        return "selector field given a string — typed ctor such as face(b, \"x_max\") \
                or vertex(b, \"tip\") is the usual replacement"
            .to_owned();
    }
    if is_selector_type(expected) && is_pose_type(found) {
        // D2 pose-vs-set: the fixed hint substring task 4833's fixtures assert.
        return "selector field given a coordinate pose — a pose locates a datum, \
                it does not name a region target"
            .to_owned();
    }
    if is_dimensioned_scalar(expected) && (found == "Real" || found == "Int") {
        // D4-6 dimensioned-scalar migration family.
        return "dimensioned scalar field given a bare number — a dimensioned \
                literal (e.g. 1m/s) is the usual replacement"
            .to_owned();
    }
    if expected == "String" && (found == "Int" || found == "Real" || found == "Bool") {
        return "string field given a non-string literal".to_owned();
    }
    NO_HINT.to_owned()
}

#[test]
fn scan_structure_defs_reads_only_the_listed_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\nstructure def Alpha { param a : Real }\nstructure def Beta { }\n",
    )
    .expect("write fea.ri");
    std::fs::write(
        dir.path().join("joints.ri"),
        "module std.joints\nstructure def Gamma { }\n",
    )
    .expect("write joints.ri");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert_eq!(
        defs.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Alpha", "Beta"],
        "only the LISTED module's defs may enter the FEA partition"
    );
    assert!(
        !defs.contains("Gamma"),
        "an unlisted module's defs must not be classified as FEA-owned"
    );
}

#[test]
fn scan_structure_defs_ignores_structure_def_prose_inside_comments() {
    // Measured, not hypothetical: `stdlib/fea_multi_case.ri` line 292 contains
    // the comment "// is a strict relaxation for PointLoad (its structure def
    // already declares". A naive substring scan harvests `already` as a def
    // name and would mis-classify any site whose def is literally named that.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\n\
         // its structure def already declares point and force\n\
         structure def Real1 { }\n\
             structure def Indented { }\n",
    )
    .expect("write");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert!(
        !defs.contains("already"),
        "prose inside a comment must not enter the def set, got {defs:?}"
    );
    assert!(defs.contains("Real1"), "a real column-0 def must be found");
    assert!(
        defs.contains("Indented"),
        "an INDENTED declaration must be found too: the scanner serves the inline-Rust \
         corpus half, where a `structure def` is routinely indented to match the \
         surrounding Rust. Anchoring at column 0 hid 480 such declarations and demoted \
         every site constructing them to `Owner::UnresolvedDef`. Got {defs:?}"
    );
}

#[test]
#[should_panic(expected = "fea_nonexistent")]
fn scan_structure_defs_panics_when_a_listed_module_is_missing() {
    // A stdlib rename must not silently EMPTY the do-not-touch partition and
    // mis-classify every deferred site as touchable. Fail loud instead.
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = scan_structure_defs(dir.path(), &["fea_nonexistent"]);
}

#[test]
fn fea_owned_defs_scans_the_real_stdlib() {
    let defs = fea_owned_defs();
    assert!(
        !defs.is_empty(),
        "the real FEA stdlib declares structure defs"
    );
    for expected in ["PointLoad", "FixedSupport", "LoadCase", "PressureLoad"] {
        assert!(
            defs.contains(expected),
            "'{expected}' is declared in stdlib/fea_multi_case.ri and must be FEA-owned; got {} defs",
            defs.len()
        );
    }
    assert!(
        !defs.contains("already"),
        "the comment-prose false positive must not reach the real scan either"
    );
}

#[test]
fn d9_owner_classifies_fea_non_fea_and_unattributed() {
    let fea = fea_owned_defs();
    let known: std::collections::BTreeSet<String> = ["Widget", "PointLoad"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    assert_eq!(
        d9_owner(Some("PointLoad"), fea, &known),
        Owner::FeaDeferredToV06,
        "D9: FEA defs are call-site-only; field-type flips stay v0.6-owned. FEA \
         ownership outranks the known-def gate, not the other way round"
    );
    assert_eq!(
        d9_owner(Some("Widget"), fea, &known),
        Owner::NonFea,
        "a KNOWN structure def that is not FEA-owned falls under D9's per-case judgment"
    );
    assert_eq!(
        d9_owner(Some("union"), fea, &known),
        Owner::UnresolvedDef,
        "`union` is a stdlib FUNCTION, not a structure def — recovery cannot tell the \
         two apart from `ident(`, so a name that resolves to no declaration must NOT \
         be sized into γ's actionable pile"
    );
    assert_eq!(
        d9_owner(None, fea, &known),
        Owner::Unknown,
        "an unattributable site must NEVER silently default into the touchable bucket"
    );
}

#[test]
fn structure_def_scanner_reads_the_visibility_prefixes() {
    let mut defs = std::collections::BTreeSet::new();
    collect_structure_defs_into(
        "pub structure def Actuator { }\n\
         priv structure def Hidden { }\n\
         structure def Plain { }\n\
         // structure def Commented { }\n\
         \x20   structure def Indented { }\n\
         let x = 1 // its structure def already declares y\n",
        &mut defs,
    );
    let got: Vec<&str> = defs.iter().map(String::as_str).collect();
    assert_eq!(
        got,
        vec!["Actuator", "Hidden", "Indented", "Plain"],
        "`pub`/`priv` prefixes are part of the declaration grammar (10 `pub structure \
         def` sites in the tracked corpus), and an INDENTED declaration is a real \
         declaration \u{2014} it is the shape Reify takes inside a Rust raw-string literal, \
         where 480 of 2,228 `crates/**/*.rs` declarations carry leading whitespace. \
         Comments and mid-line mentions are still not declarations"
    );
}

#[test]
fn stdlib_structure_defs_is_a_superset_of_the_fea_partition() {
    let all = stdlib_structure_defs();
    let fea = fea_owned_defs();
    assert!(
        !all.is_empty(),
        "the stdlib def scan must not be empty — an empty known-def set would demote \
         EVERY row to `UnresolvedDef` and empty γ's actionable group"
    );
    let missing: Vec<&String> = fea.iter().filter(|d| !all.contains(*d)).collect();
    assert!(
        missing.is_empty(),
        "the FEA modules are stdlib files, so every FEA def must also be found by the \
         whole-stdlib scan; missing: {missing:?}"
    );
}

#[test]
fn remedy_hint_is_a_pure_deterministic_function_of_the_type_pair() {
    // Same input -> same output, no I/O, no ordering dependence.
    let a = remedy_hint(Some("FaceSelector"), Some("String"));
    let b = remedy_hint(Some("FaceSelector"), Some("String"));
    assert_eq!(a, b, "remedy_hint must be deterministic");

    // Distinct recognised pairs map to DISTINCT fixed strings.
    //
    // `Frame3`, NOT `Frame(3)`: `Type::Frame(3)` Displays as `Frame3`
    // (`crates/reify-core/src/ty.rs`, pinned by that crate's own test) and
    // `is_pose_type` requires the dimension digit immediately after the prefix.
    // An earlier draft passed `"Frame(3)"` here, which yields `(3)` after the
    // prefix strip and is therefore NOT a pose — so `pose_at_selector` silently
    // held NO_HINT and every assertion below about it was vacuous.
    let string_at_selector = remedy_hint(Some("FaceSelector"), Some("String"));
    let pose_at_selector = remedy_hint(Some("FaceSelector"), Some("Frame3"));
    let bare_at_dimensioned = remedy_hint(Some("Scalar[m·s^-1]"), Some("Real"));
    assert_ne!(string_at_selector, pose_at_selector);
    assert_ne!(string_at_selector, bare_at_dimensioned);
    assert_ne!(pose_at_selector, bare_at_dimensioned);
    for h in [&string_at_selector, &pose_at_selector, &bare_at_dimensioned] {
        assert!(!h.is_empty(), "a recognised pair must produce a hint");
    }

    // An unrecognised pair, and a pair with a missing half, get a NEUTRAL
    // string — never an invented remedy.
    let neutral = remedy_hint(None, None);
    assert_eq!(remedy_hint(Some("Widget"), Some("Gadget")), neutral);
    assert_eq!(remedy_hint(Some("FaceSelector"), None), neutral);
    assert_eq!(remedy_hint(None, Some("String")), neutral);
    assert_ne!(
        neutral, string_at_selector,
        "the neutral string must be distinguishable from a real hint"
    );
    assert_ne!(
        neutral, pose_at_selector,
        "the D2 pose arm must produce a REAL hint, not the neutral fallback — \
         without this the pose fixture above can silently degrade to NO_HINT again \
         and every `assert_ne!` naming it still passes"
    );
}

#[test]
fn selector_type_renderings_match_what_reify_core_actually_displays() {
    use reify_core::Type;
    use reify_core::ty::SelectorKind;

    // Pin the table against the REAL Display impl by constructing types and
    // rendering them, rather than hand-transcribing wire forms. An earlier
    // draft matched `Selector(Face)` — a string the compiler never emits — so
    // every real selector site fell through to the neutral hint. Constructing
    // the values here means a Display rename goes RED instead of silently
    // re-emptying the selector arm.
    for kind in [
        SelectorKind::Face,
        SelectorKind::Edge,
        SelectorKind::Vertex,
        SelectorKind::Body,
    ] {
        let rendered = Type::Selector(kind).to_string();
        assert!(
            is_selector_type(&rendered),
            "Type::Selector({kind:?}) renders as {rendered:?}, which is_selector_type \
             does not recognise"
        );
    }
    let any = Type::AnySelector.to_string();
    assert!(
        is_selector_type(&any),
        "Type::AnySelector renders as {any:?}, which is_selector_type does not recognise"
    );

    // And the D3 case end-to-end: a String at a selector-typed field must get
    // the typed-ctor hint, not the neutral fallback.
    let hint = remedy_hint(Some(&any), Some("String"));
    assert_ne!(
        hint, NO_HINT,
        "the D3 String→selector case is the PRD's headline illegality; it must \
         carry a hint"
    );
    assert!(
        hint.contains("face(b"),
        "the hint names the typed-ctor replacement"
    );

    // Pose Display forms are `Frame3` / `Transform3` / `Point3<Length>`.
    for pose in [
        Type::Frame(3).to_string(),
        Type::Transform(3).to_string(),
        Type::point3(Type::length()).to_string(),
    ] {
        assert!(
            is_pose_type(&pose),
            "{pose:?} must be recognised as a coordinate pose"
        );
        assert_ne!(
            remedy_hint(Some(&any), Some(&pose)),
            NO_HINT,
            "the D2 pose-vs-set case must carry a hint for {pose:?}"
        );
    }

    // …and the NEGATIVE half, which the true-positive loop above cannot catch:
    // `Type::StructureRef(name)` Displays as the BARE struct name, so a
    // prefix-only `is_pose_type` would call these poses. `PointLoad` is a real
    // FEA def (PRD §4 D9 names it), `PointCloud` is a real def in this tree, and
    // both would then carry the D2 "a pose locates a datum" hint — a confidently
    // wrong remedy inside the do-not-touch partition.
    //
    // The last three pin the OTHER boundary, one step in from the bare-prefix
    // one: a digit-only guard admits every one of them. `Point3D`, `Point2Ref`
    // and `Frame4Bar` are legal struct names that carry a digit immediately
    // after a pose prefix, and no such def exists in the corpus today — so the
    // bug would have been latent until one landed, and then silent. A pose's
    // dimension is followed by end-of-string or `<`, never by more name.
    for not_a_pose in [
        "PointLoad",
        "PointCloud",
        "Framework",
        "Transformer",
        "Point3D",
        "Point2Ref",
        "Frame4Bar",
    ] {
        let rendered = Type::StructureRef(not_a_pose.into()).to_string();
        assert_eq!(
            rendered, not_a_pose,
            "Type::StructureRef must still Display as the bare struct name; if that \
             changed, this negative case is testing the wrong string"
        );
        assert!(
            !is_pose_type(&rendered),
            "{rendered:?} is a structure ref, not a coordinate pose — a bare-prefix \
             match here puts a false D2 remedy hint on real rows"
        );
        assert_eq!(
            remedy_hint(Some(&any), Some(&rendered)),
            NO_HINT,
            "a structure ref at a selector-typed field has no mechanical remedy; the \
             neutral fallback is the honest answer"
        );
    }
}
