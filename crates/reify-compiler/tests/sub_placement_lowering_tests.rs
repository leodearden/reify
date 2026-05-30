//! Sub placement lowering tests (task 3900).
//!
//! Exercises that `at` pose clauses and `aux` modifiers on `sub` declarations
//! are correctly lowered into `SubComponentDecl.pose` /
//! `SubComponentDecl.is_aux` in the compiled IR.
//!
//! All tests use the `parse->compile->inspect` pattern against
//! `reify_test_support::compile_source_with_stdlib` — stdlib builtins
//! (transform3/orient_identity/vec3) must be resolvable by the compiler.

// ── Step 1: SubComponentDecl.pose / is_aux ───────────────────────────────────

/// `aux sub … at <pose>` lowers to `is_aux = true` and `pose = Some(…)`.
#[test]
fn aux_sub_lowers_pose_and_is_aux() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    aux sub jig : Child at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");

    let jig = parent
        .sub_components
        .iter()
        .find(|s| s.name == "jig")
        .expect("sub 'jig' not found in Parent.sub_components");

    assert!(
        jig.pose.is_some(),
        "expected jig.pose to be Some(…) after `at` lowering"
    );
    assert!(jig.is_aux, "expected jig.is_aux = true for `aux sub`");
}

/// A plain `sub` without `aux` or `at` lowers to `is_aux = false`, `pose = None`.
#[test]
fn plain_sub_has_no_pose_not_aux() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    sub plate : Child
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");

    let plate = parent
        .sub_components
        .iter()
        .find(|s| s.name == "plate")
        .expect("sub 'plate' not found in Parent.sub_components");

    assert!(
        plate.pose.is_none(),
        "expected plate.pose = None for plain sub"
    );
    assert!(
        !plate.is_aux,
        "expected plate.is_aux = false for plain sub"
    );
}

// ── Step 3: ValueCellDecl.is_aux and pub⊥aux orthogonality ──────────────────

/// `aux let` lowers to `is_aux = true` on the ValueCellDecl.
#[test]
fn aux_let_sets_is_aux() {
    let source = r#"structure S {
    aux let blank = 5mm
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let s = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    let blank = s
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "blank")
        .expect("let 'blank' not found in S.value_cells");

    assert!(blank.is_aux, "expected blank.is_aux = true for `aux let`");
}

/// `pub` and `aux` are orthogonal axes — all four (visibility × is_aux) combos
/// are independently representable in the IR.
#[test]
fn aux_and_pub_are_independent() {
    let source = r#"structure S {
    pub aux let a = 1mm
    aux let b = 1mm
    pub let c = 1mm
    let d = 1mm
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let s = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    let find_cell = |name: &str| {
        s.value_cells
            .iter()
            .find(|vc| vc.id.member == name)
            .unwrap_or_else(|| panic!("let '{}' not found in S.value_cells", name))
    };

    let a = find_cell("a");
    let b = find_cell("b");
    let c = find_cell("c");
    let d = find_cell("d");

    // pub aux: exported AND auxiliary
    assert_eq!(
        a.visibility,
        reify_compiler::Visibility::Public,
        "a should be Public"
    );
    assert!(a.is_aux, "a should be is_aux=true");

    // aux (no pub): private AND auxiliary
    assert_eq!(
        b.visibility,
        reify_compiler::Visibility::Private,
        "b should be Private"
    );
    assert!(b.is_aux, "b should be is_aux=true");

    // pub (no aux): exported, not auxiliary
    assert_eq!(
        c.visibility,
        reify_compiler::Visibility::Public,
        "c should be Public"
    );
    assert!(!c.is_aux, "c should be is_aux=false");

    // plain let: private, not auxiliary
    assert_eq!(
        d.visibility,
        reify_compiler::Visibility::Private,
        "d should be Private"
    );
    assert!(!d.is_aux, "d should be is_aux=false");
}
