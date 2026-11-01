//! The reify language's one keyword table: each word's grammar kind and its LSP rename policy
//! (PRD docs/prds/v0_6/builtin-registry-migration-and-interception-classifier.md D15).

use RenamePolicy::{Accepted, Refused};

/// One row of [`KEYWORDS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyword {
    pub word: &'static str,
    pub kind: KeywordKind,
}

/// How the grammar treats a keyword where a binding name is expected. The kind is measured by
/// parsing each word as a binding name, never assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
    /// Cannot be a binding name.
    Reserved,
    /// Parses as a binding name; a keyword only in specific grammar positions.
    Contextual { rename: RenamePolicy },
}

/// Whether the LSP rename gate accepts a `Contextual` keyword as a new binding name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenamePolicy {
    Refused,
    Accepted,
}

impl Keyword {
    /// True when the LSP rename gate must refuse this word as a new binding name: every
    /// `Reserved` row, plus each `Contextual` row whose policy is `Refused`.
    pub const fn rename_refused(&self) -> bool {
        matches!(
            self.kind,
            KeywordKind::Reserved
                | KeywordKind::Contextual {
                    rename: RenamePolicy::Refused
                }
        )
    }
}

/// The [`KEYWORDS`] row for `word` (exact, case-sensitive), or `None` for a non-keyword.
pub fn lookup_keyword(word: &str) -> Option<&'static Keyword> {
    KEYWORDS.iter().find(|keyword| keyword.word == word)
}

const fn reserved(word: &'static str) -> Keyword {
    Keyword {
        word,
        kind: KeywordKind::Reserved,
    }
}

const fn contextual(word: &'static str, rename: RenamePolicy) -> Keyword {
    Keyword {
        word,
        kind: KeywordKind::Contextual { rename },
    }
}

/// Every reify keyword; the word set and each row's kind are pinned by this module's tests (PRD D15).
pub const KEYWORDS: &[Keyword] = &[
    contextual("and", Refused),
    contextual("as", Accepted),
    reserved("auto"),
    contextual("chain", Accepted),
    contextual("connect", Refused),
    contextual("constraint", Refused),
    contextual("def", Refused),
    contextual("else", Refused),
    contextual("enum", Refused),
    reserved("exists"),
    reserved("false"),
    contextual("field", Accepted),
    contextual("fn", Refused),
    reserved("forall"),
    reserved("if"),
    contextual("implies", Accepted),
    contextual("import", Refused),
    contextual("in", Accepted),
    contextual("let", Refused),
    reserved("map"),
    reserved("match"),
    contextual("maximize", Refused),
    contextual("meta", Accepted),
    contextual("minimize", Refused),
    contextual("module", Accepted),
    contextual("none", Accepted),
    reserved("not"),
    contextual("occurrence", Refused),
    contextual("or", Refused),
    contextual("out", Accepted),
    contextual("param", Refused),
    contextual("port", Refused),
    contextual("pub", Accepted),
    contextual("purpose", Refused),
    contextual("self", Accepted),
    reserved("set"),
    contextual("some", Accepted),
    contextual("structure", Refused),
    contextual("sub", Refused),
    contextual("then", Refused),
    contextual("trait", Refused),
    reserved("true"),
    contextual("type", Refused),
    reserved("undef"),
    contextual("unit", Accepted),
    contextual("where", Refused),
    contextual("joint", Accepted),
];

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ast::{Declaration, ExprKind, MemberDecl};
    use std::collections::BTreeSet;
    use std::path::Path;

    const GRAMMAR_ONLY: &[&str] = &["joint"];

    const RULED_REFUSED_CONTEXTUAL: &[&str] = &[
        "and",
        "connect",
        "constraint",
        "def",
        "else",
        "enum",
        "fn",
        "import",
        "let",
        "maximize",
        "minimize",
        "occurrence",
        "or",
        "param",
        "port",
        "purpose",
        "structure",
        "sub",
        "then",
        "trait",
        "type",
        "where",
    ];

    fn binding_probe(w: &str) -> String {
        format!(
            "structure S {{\n    param {w}: Real = 1\n    let u0 = {w}\n    let u1 = {w} + 1\n    let u2 = 1 + {w}\n    let u3 = max({w}, 2)\n    let u4 = [{w}, 1]\n    constraint {w} > 0\n}}\n"
        )
    }

    fn parses_as_binding_name(w: &str) -> bool {
        let module = crate::parse(&binding_probe(w), reify_core::ModulePath::single("probe"));
        if !module.errors.is_empty() {
            return false;
        }
        let Some(Declaration::Structure(structure)) = module.declarations.first() else {
            return false;
        };
        let declares_param = structure
            .members
            .iter()
            .any(|member| matches!(member, MemberDecl::Param(p) if p.name == w));
        let u0_reads_the_binding = structure.members.iter().any(|member| {
            matches!(
                member,
                MemberDecl::Let(l)
                    if l.name == "u0" && matches!(&l.value.kind, ExprKind::Ident(n) if n == w)
            )
        });
        declares_param && u0_reads_the_binding
    }

    fn words(filter: impl Fn(&Keyword) -> bool) -> BTreeSet<&'static str> {
        KEYWORDS
            .iter()
            .filter(|k| filter(k))
            .map(|k| k.word)
            .collect()
    }

    #[test]
    fn keyword_kinds_match_parse_behaviour() {
        assert!(
            parses_as_binding_name("width"),
            "control identifier `width` must parse as a binding name; the probe is broken:\n{}",
            binding_probe("width")
        );
        assert!(
            KEYWORDS.iter().any(|k| k.kind == KeywordKind::Reserved),
            "KEYWORDS has no Reserved row, so the parse-behaviour check is vacuous"
        );
        let mismatches: Vec<String> = KEYWORDS
            .iter()
            .filter_map(|k| {
                let parses = parses_as_binding_name(k.word);
                let contextual = matches!(k.kind, KeywordKind::Contextual { .. });
                (parses != contextual).then(|| {
                    format!(
                        "`{}`: row kind {:?}, parses as a binding name = {parses}",
                        k.word, k.kind
                    )
                })
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "KEYWORDS kinds disagree with the parser (the parse is the authority, PRD D15):\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn keyword_words_match_spec_section_2_10() {
        const START: &str = "### 2.10 Keywords";
        const END: &str = "**Removed keywords";
        let spec_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reify-language-spec.md");
        let spec = std::fs::read_to_string(&spec_path).unwrap_or_else(|e| {
            panic!(
                "cannot read the language spec at {}: {e}",
                spec_path.display()
            )
        });
        let section = spec
            .split_once(START)
            .and_then(|(_, rest)| rest.split_once(END))
            .map(|(section, _)| section)
            .unwrap_or_else(|| panic!("{} has no `{START}` … `{END}` span", spec_path.display()));
        let spec_words: BTreeSet<&str> = section
            .lines()
            .filter(|line| line.starts_with("**"))
            .flat_map(|line| line.split('`').skip(1).step_by(2))
            .collect();
        assert!(
            !spec_words.is_empty(),
            "no backticked keywords found between `{START}` and `{END}` in {}",
            spec_path.display()
        );

        let listed_by_spec: Vec<&str> = GRAMMAR_ONLY
            .iter()
            .copied()
            .filter(|w| spec_words.contains(w))
            .collect();
        assert!(
            listed_by_spec.is_empty(),
            "spec §2.10 now lists {listed_by_spec:?}; drop them from GRAMMAR_ONLY"
        );

        let expected: BTreeSet<&str> = spec_words.iter().chain(GRAMMAR_ONLY).copied().collect();
        let table = words(|_| true);
        assert_eq!(
            table,
            expected,
            "KEYWORDS words differ from spec §2.10 ∪ {GRAMMAR_ONLY:?}\n  only in KEYWORDS: {:?}\n  missing from KEYWORDS: {:?}",
            table.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&table).collect::<Vec<_>>()
        );
    }

    #[test]
    fn keyword_words_are_unique() {
        let mut seen = BTreeSet::new();
        let duplicates: Vec<&str> = KEYWORDS
            .iter()
            .map(|k| k.word)
            .filter(|w| !seen.insert(*w))
            .collect();
        assert!(duplicates.is_empty(), "KEYWORDS repeats {duplicates:?}");
    }

    #[test]
    fn rename_refused_set_is_reserved_plus_the_ruled_contextual_list() {
        let ruled: BTreeSet<&str> = RULED_REFUSED_CONTEXTUAL.iter().copied().collect();
        let refused_contextual = words(|k| {
            k.kind
                == KeywordKind::Contextual {
                    rename: RenamePolicy::Refused,
                }
        });
        assert_eq!(
            refused_contextual, ruled,
            "the rename-refused Contextual rows must be exactly the ruled list"
        );

        let refused = words(Keyword::rename_refused);
        let expected: BTreeSet<&str> = words(|k| k.kind == KeywordKind::Reserved)
            .union(&ruled)
            .copied()
            .collect();
        assert!(!refused.is_empty(), "no KEYWORDS row is rename-refused");
        assert_eq!(
            refused, expected,
            "rename_refused() must hold for exactly the Reserved rows plus the ruled Contextual list"
        );
    }

    #[test]
    fn lookup_keyword_finds_rows_and_rejects_non_keywords() {
        for word in ["let", "match"] {
            assert!(
                lookup_keyword(word).is_some(),
                "`{word}` must be a KEYWORDS row"
            );
        }
        for word in ["width", "Let", "derived", "point3"] {
            assert_eq!(
                lookup_keyword(word),
                None,
                "`{word}` must not be a KEYWORDS row"
            );
        }
        assert!(
            lookup_keyword("match").is_some_and(Keyword::rename_refused),
            "`match` is Reserved, so rename must be refused"
        );
    }
}
