//! Phase-13 module content hash.
//!
//! Combines every piece of owned compiled content from `CompilationCtx`
//! plus `compiled_purposes`, `parsed.path`, and `parsed.pragmas` into a single
//! [`ContentHash`] via `ContentHash::combine_all`. Type-alias hashes are
//! sorted by name so alias-declaration order doesn't perturb the result.
//! Module pragmas are appended last (in declaration order) so pragma-free
//! modules retain identical hashes to pre-pragma-hashing compilations.

use reify_ast::ParsedModule;
use reify_core::ContentHash;
use reify_ir::CompiledFunction;

use crate::compile_builder::ctx::CompilationCtx;
use crate::types::CompiledPurpose;

/// Compute the module-level content hash by combining the path with every
/// piece of compiled content owned by `ctx` plus the external
/// `compiled_purposes` slice.
pub(crate) fn compute_module_hash(
    ctx: &CompilationCtx,
    parsed: &ParsedModule,
    compiled_purposes: &[CompiledPurpose],
) -> ContentHash {
    let path_hash = ContentHash::of_str(&parsed.path.to_string());

    // Template content hashes
    let template_hashes = ctx.templates.iter().map(|t| t.content_hash);

    // Import path hashes
    let import_hashes = ctx.imports.iter().map(|i| ContentHash::of_str(&i.path));

    // Enum def hashes
    let enum_hashes = ctx.enum_defs.iter().map(|e| {
        let mut h = ContentHash::of_str(&e.name);
        for v in &e.variants {
            h = h.combine(ContentHash::of_str(v));
        }
        h
    });

    // Function content hashes
    let function_hashes = ctx
        .functions
        .iter()
        .map(|f: &CompiledFunction| f.content_hash);

    // Trait content hashes
    let trait_hashes = ctx.trait_defs.iter().map(|t| t.content_hash);

    // Field content hashes
    let field_hashes = ctx.fields.iter().map(|f| f.content_hash);

    // Purpose content hashes
    let purpose_hashes = compiled_purposes.iter().map(|p| p.content_hash);

    // Unit content hashes
    let unit_hashes = ctx.compiled_units.iter().map(|u| u.content_hash);

    // Type alias content hashes (sorted by name for deterministic ordering)
    let mut alias_hash_pairs: Vec<_> = ctx
        .alias_registry
        .iter()
        .map(|a| (a.name.clone(), a.content_hash))
        .collect();
    alias_hash_pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let alias_hashes = alias_hash_pairs.into_iter().map(|(_, h)| h);

    // Module-level pragma hashes (in declaration order; span excluded as positional).
    let pragma_hashes = parsed.pragmas.iter().map(hash_pragma);

    let all_hashes = std::iter::once(path_hash)
        .chain(template_hashes)
        .chain(import_hashes)
        .chain(enum_hashes)
        .chain(function_hashes)
        .chain(trait_hashes)
        .chain(field_hashes)
        .chain(purpose_hashes)
        .chain(unit_hashes)
        .chain(alias_hashes)
        .chain(pragma_hashes);

    ContentHash::combine_all(all_hashes)
}

/// Produce a deterministic [`ContentHash`] for a single module-level pragma.
///
/// Combines the pragma name hash with a u64-encoded argument count, then combines
/// each argument's kind, key (for key-value), and value in declaration order.
/// Encoding the count explicitly guards against collisions when a pragma has
/// optional arguments that could be absent vs. present as an empty value.
/// Source span is intentionally excluded — it is positional metadata, not content.
pub(crate) fn hash_pragma(p: &reify_ast::Pragma) -> ContentHash {
    let mut h = ContentHash::of_str(&p.name).combine(ContentHash::of_u64(p.args.len() as u64));
    for arg in &p.args {
        h = h.combine(hash_pragma_arg(arg));
    }
    h
}

fn hash_pragma_arg(arg: &reify_ast::PragmaArg) -> ContentHash {
    match arg {
        reify_ast::PragmaArg::KeyValue { key, value } => ContentHash::of_str("kv")
            .combine(ContentHash::of_str(key))
            .combine(hash_pragma_value(value)),
        reify_ast::PragmaArg::Bare(value) => {
            ContentHash::of_str("bare").combine(hash_pragma_value(value))
        }
    }
}

fn hash_pragma_value(v: &reify_ast::PragmaValue) -> ContentHash {
    match v {
        reify_ast::PragmaValue::Ident(s) => {
            ContentHash::of_str("ident").combine(ContentHash::of_str(s))
        }
        reify_ast::PragmaValue::Number(n) => {
            ContentHash::of_str("num").combine(ContentHash::of_u64(n.to_bits()))
        }
        reify_ast::PragmaValue::String(s) => {
            ContentHash::of_str("str").combine(ContentHash::of_str(s))
        }
        reify_ast::PragmaValue::Bool(b) => ContentHash::of_str("bool")
            .combine(ContentHash::of_str(if *b { "true" } else { "false" })),
        reify_ast::PragmaValue::Quantity { value, unit } => ContentHash::of_str("quantity")
            .combine(ContentHash::of_u64(value.to_bits()))
            .combine(ContentHash::of_str(unit)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Ident("true")` and `Bool(true)` share the same payload text but must
    /// hash differently because their kind-tags differ (`"ident"` vs `"bool"`).
    #[test]
    fn pragma_value_ident_vs_bool_differ() {
        let h_ident = hash_pragma_value(&reify_ast::PragmaValue::Ident("true".to_string()));
        let h_bool = hash_pragma_value(&reify_ast::PragmaValue::Bool(true));
        assert_ne!(
            h_ident, h_bool,
            "Ident(\"true\") and Bool(true) must produce distinct hashes"
        );
    }

    /// `Ident("42")` and `Number(42.0)` share numerically equal payloads but
    /// must hash differently because their kind-tags differ (`"ident"` vs `"num"`).
    #[test]
    fn pragma_value_ident_vs_number_differ() {
        let h_ident = hash_pragma_value(&reify_ast::PragmaValue::Ident("42".to_string()));
        let h_num = hash_pragma_value(&reify_ast::PragmaValue::Number(42.0_f64));
        assert_ne!(
            h_ident, h_num,
            "Ident(\"42\") and Number(42) must produce distinct hashes"
        );
    }

    /// `Bool(true)` and `String("true")` share the same payload text but must
    /// hash differently because their kind-tags differ (`"bool"` vs `"str"`).
    #[test]
    fn pragma_value_bool_vs_string_differ() {
        let h_bool = hash_pragma_value(&reify_ast::PragmaValue::Bool(true));
        let h_str = hash_pragma_value(&reify_ast::PragmaValue::String("true".to_string()));
        assert_ne!(
            h_bool, h_str,
            "Bool(true) and String(\"true\") must produce distinct hashes"
        );
    }

    /// Pins the arg-level discriminator collision invariant: `Bare(v)` and
    /// `KeyValue { key: "", value: v }` must hash differently even when the key
    /// is empty and the value is identical.
    ///
    /// With an empty key the KeyValue branch reduces to:
    ///   `ContentHash::of_str("kv") + of_str("") + hash_pragma_value(v)`
    /// while Bare reduces to:
    ///   `ContentHash::of_str("bare") + hash_pragma_value(v)`
    ///
    /// The test guards against a format-string regression that drops the leading
    /// "bare"/"kv" kind-tag, which would cause a zero-length-key KeyValue to
    /// collide with the Bare variant and silently invalidate cache keys.
    #[test]
    fn pragma_arg_bare_vs_keyvalue_differ_for_same_value() {
        let v = reify_ast::PragmaValue::Ident("x".to_string());
        let h_bare = hash_pragma_arg(&reify_ast::PragmaArg::Bare(v.clone()));
        let h_kv = hash_pragma_arg(&reify_ast::PragmaArg::KeyValue {
            key: "".to_string(),
            value: v,
        });
        assert_ne!(
            h_bare, h_kv,
            "Bare(v) and KeyValue {{ key: \"\", value: v }} must produce distinct hashes \
             — the 'bare'/'kv' kind-tag prefixes must diverge even when the key is empty \
             and values match"
        );
    }

    /// Pins the pragma name/arg-count encoding invariant: a zero-arg pragma and a
    /// one-arg pragma with the same name must produce distinct hashes.
    ///
    /// `hash_pragma` encodes `p.args.len()` as a u64 explicitly so that a pragma
    /// with optional arguments absent (`args: vec![]`) cannot collide with the same
    /// pragma name where the optional argument is present as an empty value.
    ///
    /// > "Encoding the count explicitly guards against collisions when a pragma has
    /// > optional arguments that could be absent vs. present as an empty value."
    /// > — hash_pragma docstring
    ///
    /// Span is set to `SourceSpan::new(0, 0)` because span is positional metadata
    /// explicitly excluded from the hash (per the docstring), so span choice is
    /// irrelevant to the assertion.
    #[test]
    fn pragma_arg_count_encoded_distinctly() {
        let p_empty = reify_ast::Pragma {
            name: "foo".to_string(),
            args: vec![],
            span: reify_core::SourceSpan::new(0, 0),
        };
        let p_one = reify_ast::Pragma {
            name: "foo".to_string(),
            args: vec![reify_ast::PragmaArg::Bare(
                reify_ast::PragmaValue::Ident("x".to_string()),
            )],
            span: reify_core::SourceSpan::new(0, 0),
        };
        assert_ne!(
            hash_pragma(&p_empty),
            hash_pragma(&p_one),
            "zero-arg and one-arg pragmas with the same name must hash distinctly \
             — arg count is encoded to prevent optional-absent vs empty-present collisions"
        );
    }
}
