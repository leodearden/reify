//! Kleene three-valued logic helpers — single source of truth for §9.2.3 of
//! `docs/reify-language-spec.md`.
//!
//! The three truth values are [`KBool::True`], [`KBool::False`], and
//! [`KBool::Undef`] (unknown/indeterminate).  The three operators implement the
//! truth tables specified in §9.2.3 exactly:
//!
//! | a     | b     | AND   | OR    |
//! |-------|-------|-------|-------|
//! | T     | T     | T     | T     |
//! | T     | F     | F     | T     |
//! | T     | U     | U     | T     |
//! | F     | T     | F     | T     |
//! | F     | F     | F     | F     |
//! | F     | U     | F     | U     |
//! | U     | T     | U     | T     |
//! | U     | F     | F     | U     |
//! | U     | U     | U     | U     |
//!
//! | a     | NOT a |
//! |-------|-------|
//! | T     | F     |
//! | F     | T     |
//! | U     | U     |
//!
//! Use [`KBool::try_from`] to convert from a [`reify_types::Value`] (returns
//! `Err(())` for non-bool, non-undef variants so callers can preserve the
//! existing "type-error → `Value::Undef`" catch-all), and [`Value::from`] to
//! convert back.

use reify_ir::Value;

/// A three-valued Kleene truth value.
///
/// Corresponds directly to the three logical states defined in
/// `docs/reify-language-spec.md` §9.2.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KBool {
    /// Definitely true.
    True,
    /// Definitely false.
    False,
    /// Unknown / indeterminate (maps to [`Value::Undef`]).
    Undef,
}

/// Kleene three-valued AND.
///
/// `False` is the absorbing element: `False ∧ x = False` for all `x`.
/// When neither operand is `False`, `Undef` propagates.
///
/// See `docs/reify-language-spec.md` §9.2.3.
pub fn kleene_and(a: KBool, b: KBool) -> KBool {
    match (a, b) {
        (KBool::False, _) | (_, KBool::False) => KBool::False,
        (KBool::True, KBool::True) => KBool::True,
        _ => KBool::Undef,
    }
}

/// Kleene three-valued OR.
///
/// `True` is the absorbing element: `True ∨ x = True` for all `x`.
/// When neither operand is `True`, `Undef` propagates.
///
/// See `docs/reify-language-spec.md` §9.2.3.
pub fn kleene_or(a: KBool, b: KBool) -> KBool {
    match (a, b) {
        (KBool::True, _) | (_, KBool::True) => KBool::True,
        (KBool::False, KBool::False) => KBool::False,
        _ => KBool::Undef,
    }
}

/// Kleene three-valued NOT.
///
/// `¬True = False`, `¬False = True`, `¬Undef = Undef`.
///
/// See `docs/reify-language-spec.md` §9.2.3.
pub fn kleene_not(a: KBool) -> KBool {
    match a {
        KBool::True => KBool::False,
        KBool::False => KBool::True,
        KBool::Undef => KBool::Undef,
    }
}

impl TryFrom<&Value> for KBool {
    type Error = ();

    /// Convert a [`Value`] to a [`KBool`].
    ///
    /// - `Bool(true)` → `Ok(True)`
    /// - `Bool(false)` → `Ok(False)`
    /// - `Undef` → `Ok(Undef)`
    /// - any other variant → `Err(())`
    fn try_from(v: &Value) -> Result<Self, ()> {
        match v {
            Value::Bool(true) => Ok(KBool::True),
            Value::Bool(false) => Ok(KBool::False),
            Value::Undef => Ok(KBool::Undef),
            _ => Err(()),
        }
    }
}

impl From<KBool> for Value {
    /// Convert a [`KBool`] back to a [`Value`].
    ///
    /// - `True` → `Value::Bool(true)`
    /// - `False` → `Value::Bool(false)`
    /// - `Undef` → `Value::Undef`
    fn from(k: KBool) -> Value {
        match k {
            KBool::True => Value::Bool(true),
            KBool::False => Value::Bool(false),
            KBool::Undef => Value::Undef,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // kleene_and: all 9 rows of the §9.2.3 truth table
    // -----------------------------------------------------------------------

    #[test]
    fn kleene_and_truth_table() {
        use KBool::*;
        // T ∧ T = T
        assert_eq!(kleene_and(True, True), True);
        // T ∧ F = F
        assert_eq!(kleene_and(True, False), False);
        // T ∧ U = U
        assert_eq!(kleene_and(True, Undef), Undef);
        // F ∧ T = F
        assert_eq!(kleene_and(False, True), False);
        // F ∧ F = F
        assert_eq!(kleene_and(False, False), False);
        // F ∧ U = F  (absorbing element)
        assert_eq!(kleene_and(False, Undef), False);
        // U ∧ T = U
        assert_eq!(kleene_and(Undef, True), Undef);
        // U ∧ F = F  (absorbing element)
        assert_eq!(kleene_and(Undef, False), False);
        // U ∧ U = U
        assert_eq!(kleene_and(Undef, Undef), Undef);
    }

    // -----------------------------------------------------------------------
    // kleene_or: all 9 rows of the §9.2.3 truth table
    // -----------------------------------------------------------------------

    #[test]
    fn kleene_or_truth_table() {
        use KBool::*;
        // T ∨ T = T
        assert_eq!(kleene_or(True, True), True);
        // T ∨ F = T
        assert_eq!(kleene_or(True, False), True);
        // T ∨ U = T  (absorbing element)
        assert_eq!(kleene_or(True, Undef), True);
        // F ∨ T = T
        assert_eq!(kleene_or(False, True), True);
        // F ∨ F = F
        assert_eq!(kleene_or(False, False), False);
        // F ∨ U = U
        assert_eq!(kleene_or(False, Undef), Undef);
        // U ∨ T = T  (absorbing element)
        assert_eq!(kleene_or(Undef, True), True);
        // U ∨ F = U
        assert_eq!(kleene_or(Undef, False), Undef);
        // U ∨ U = U
        assert_eq!(kleene_or(Undef, Undef), Undef);
    }

    // -----------------------------------------------------------------------
    // kleene_implies: all 9 rows of the §9.2.3 truth table (a ⇒ b)
    // -----------------------------------------------------------------------

    #[test]
    fn kleene_implies_truth_table() {
        use KBool::*;
        // T ⇒ T = T
        assert_eq!(kleene_implies(True, True), True);
        // T ⇒ F = F
        assert_eq!(kleene_implies(True, False), False);
        // T ⇒ U = U
        assert_eq!(kleene_implies(True, Undef), Undef);
        // F ⇒ T = T  (vacuous)
        assert_eq!(kleene_implies(False, True), True);
        // F ⇒ F = T  (vacuous)
        assert_eq!(kleene_implies(False, False), True);
        // F ⇒ U = T  (vacuous: ¬False = True is absorbing for OR)
        assert_eq!(kleene_implies(False, Undef), True);
        // U ⇒ T = T  (¬Undef = Undef, Undef ∨ True = True)
        assert_eq!(kleene_implies(Undef, True), True);
        // U ⇒ F = U  (¬Undef = Undef, Undef ∨ False = Undef)
        assert_eq!(kleene_implies(Undef, False), Undef);
        // U ⇒ U = U  (¬Undef = Undef, Undef ∨ Undef = Undef)
        assert_eq!(kleene_implies(Undef, Undef), Undef);
    }

    // -----------------------------------------------------------------------
    // kleene_not: all 3 rows
    // -----------------------------------------------------------------------

    #[test]
    fn kleene_not_truth_table() {
        use KBool::*;
        assert_eq!(kleene_not(True), False);
        assert_eq!(kleene_not(False), True);
        assert_eq!(kleene_not(Undef), Undef);
    }

    // -----------------------------------------------------------------------
    // TryFrom<&Value> for KBool
    // -----------------------------------------------------------------------

    #[test]
    fn try_from_value_bool_true() {
        let v = Value::Bool(true);
        assert_eq!(KBool::try_from(&v), Ok(KBool::True));
    }

    #[test]
    fn try_from_value_bool_false() {
        let v = Value::Bool(false);
        assert_eq!(KBool::try_from(&v), Ok(KBool::False));
    }

    #[test]
    fn try_from_value_undef() {
        let v = Value::Undef;
        assert_eq!(KBool::try_from(&v), Ok(KBool::Undef));
    }

    #[test]
    fn try_from_value_non_bool_is_err() {
        assert_eq!(KBool::try_from(&Value::Int(3)), Err(()));
        assert_eq!(KBool::try_from(&Value::Real(0.0)), Err(()));
    }

    // -----------------------------------------------------------------------
    // From<KBool> for Value
    // -----------------------------------------------------------------------

    #[test]
    fn from_kbool_into_value() {
        assert_eq!(Value::from(KBool::True), Value::Bool(true));
        assert_eq!(Value::from(KBool::False), Value::Bool(false));
        assert_eq!(Value::from(KBool::Undef), Value::Undef);
    }
}
