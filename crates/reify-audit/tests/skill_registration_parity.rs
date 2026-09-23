//! The `/audit` skill (`.claude/skills/audit/`) is the consumer that runs
//! `reify-audit` detectors and routes their findings. A `--pattern` token
//! missing from one of its surfaces is a detector the skill cannot run or
//! route; PDSSENTINEL, PDOCCOVER, PDIAG and PDCHECK all landed that way (#6354).
//! A `Finding.pattern` value missing from its routing registry is a finding
//! the skill cannot map back to its token's routing notes.

use reify_audit::{Pattern, pattern_flag};
use serde::Deserialize;
use serde::de::{self, Visitor};
use std::fmt;
use std::path::{Path, PathBuf};

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills/audit")
}

enum Scope {
    Frontmatter,
    Section(&'static str),
}

struct Surface {
    file: &'static str,
    scope: Scope,
    role: &'static str,
}

const INVOCATION_SURFACES: &[Surface] = &[
    Surface {
        file: "SKILL.md",
        scope: Scope::Frontmatter,
        role: "skill trigger list",
    },
    Surface {
        file: "SKILL.md",
        scope: Scope::Section("## Modes"),
        role: "invocation-mode table",
    },
    Surface {
        file: "references/modes.md",
        scope: Scope::Section("## §4 "),
        role: "pattern-restricted argv",
    },
    Surface {
        file: "references/cli-invocation.md",
        scope: Scope::Section("## §2 "),
        role: "canonical argv template",
    },
];

const ROUTING_SURFACES: &[Surface] = &[Surface {
    file: "references/severity-routing.md",
    scope: Scope::Section("## §0 "),
    role: "pattern routing registry",
}];

fn all_surfaces() -> impl Iterator<Item = &'static Surface> {
    INVOCATION_SURFACES.iter().chain(ROUTING_SURFACES)
}

impl Scope {
    fn kind(&self) -> &'static str {
        match self {
            Scope::Frontmatter => "frontmatter",
            Scope::Section(_) => "section",
        }
    }

    fn extract(&self, text: &str) -> Option<String> {
        match self {
            Scope::Frontmatter => frontmatter(text),
            Scope::Section(heading_prefix) => section(text, heading_prefix),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Frontmatter => f.write_str("frontmatter"),
            Scope::Section(heading_prefix) => write!(f, "`{heading_prefix}`"),
        }
    }
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} ({})", self.file, self.scope, self.role)
    }
}

fn frontmatter(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut body = Vec::new();
    for line in lines {
        if line == "---" {
            return Some(body.join("\n"));
        }
        body.push(line);
    }
    None
}

/// From the first line starting with `heading_prefix` up to the next `## `
/// heading. Lines inside ``` fences are never taken as headings.
fn section(text: &str, heading_prefix: &str) -> Option<String> {
    let mut in_fence = false;
    let mut body: Option<Vec<&str>> = None;
    for line in text.lines() {
        let is_fence_marker = line.trim_start().starts_with("```");
        if !in_fence && !is_fence_marker {
            match body {
                None if line.starts_with(heading_prefix) => body = Some(Vec::new()),
                Some(_) if line.starts_with("## ") => break,
                _ => {}
            }
        }
        if is_fence_marker {
            in_fence = !in_fence;
        }
        if let Some(lines) = body.as_mut() {
            lines.push(line);
        }
    }
    body.map(|lines| lines.join("\n"))
}

fn mentions_token(text: &str, token: &str) -> bool {
    let is_word_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(token).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + token.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

fn registration_gaps<'a>(
    tokens: &[&str],
    surfaces: impl IntoIterator<Item = &'a Surface>,
) -> Vec<String> {
    let mut gaps = Vec::new();
    for surface in surfaces {
        let path = skill_dir().join(surface.file);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                gaps.push(format!("{surface}: unreadable at {}: {e}", path.display()));
                continue;
            }
        };
        let Some(scope_text) = surface.scope.extract(&text) else {
            gaps.push(format!("{surface}: {} not found", surface.scope.kind()));
            continue;
        };
        gaps.extend(
            tokens
                .iter()
                .filter(|token| !mentions_token(&scope_text, token))
                .map(|token| format!("{token} missing from {surface}")),
        );
    }
    gaps
}

/// The names serde gives `T`'s variants: every value a `T` field can carry in
/// JSON. serde's derived `Deserialize` hands them to
/// `Deserializer::deserialize_enum`, where [`VariantNameProbe`] keeps them and
/// stops, so an enum is enumerated without a hand-kept list.
fn serde_variant_names<T: de::DeserializeOwned>() -> &'static [&'static str] {
    let mut names = None;
    let _ = T::deserialize(VariantNameProbe(&mut names));
    let type_name = std::any::type_name::<T>();
    names.unwrap_or_else(|| panic!("{type_name} does not deserialize as an enum"))
}

struct VariantNameProbe<'a>(&'a mut Option<&'static [&'static str]>);

impl<'de> de::Deserializer<'de> for VariantNameProbe<'_> {
    type Error = de::value::Error;

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        *self.0 = Some(variants);
        Err(de::Error::custom("variant names captured"))
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("not an enum"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

#[test]
fn every_pattern_token_is_registered_where_the_skill_invokes_detectors() {
    assert!(
        !pattern_flag::TOKENS.is_empty(),
        "reify_audit::pattern_flag::TOKENS is empty, so this parity check would pass vacuously"
    );
    let gaps = registration_gaps(pattern_flag::TOKENS, INVOCATION_SURFACES);
    assert!(
        gaps.is_empty(),
        "every `reify-audit --pattern` token must be registered wherever the /audit skill \
         invokes detectors; a token is registered by naming it on each surface listed. \
         {} gap(s):\n  {}",
        gaps.len(),
        gaps.join("\n  ")
    );
}

#[test]
fn every_pattern_token_has_a_routing_registry_row() {
    let gaps = registration_gaps(pattern_flag::TOKENS, ROUTING_SURFACES);
    assert!(
        gaps.is_empty(),
        "every `reify-audit --pattern` token needs a row in the /audit skill's routing \
         registry, which records the token's Finding.pattern values, what its \
         Finding.task_id carries and its routing notes. {} gap(s):\n  {}",
        gaps.len(),
        gaps.join("\n  ")
    );
}

#[test]
fn every_finding_pattern_value_is_in_the_routing_registry() {
    let gaps = registration_gaps(serde_variant_names::<Pattern>(), ROUTING_SURFACES);
    assert!(
        gaps.is_empty(),
        "every `Finding.pattern` value (a `reify_audit::Pattern` variant) must be named in its \
         `--pattern` token's row of the /audit skill's routing registry, which is how the skill \
         maps a finding back to that token's routing notes. {} gap(s):\n  {}",
        gaps.len(),
        gaps.join("\n  ")
    );
}

#[test]
fn serde_variant_names_lists_every_variant_in_declaration_order() {
    #[derive(Deserialize)]
    enum Probe {
        First,
        Second,
    }
    assert_eq!(serde_variant_names::<Probe>(), ["First", "Second"]);
}

#[test]
fn mentions_token_matches_whole_tokens_only() {
    assert!(mentions_token("P1|P2", "P1"));
    assert!(mentions_token("P1|P2", "P2"));
    assert!(mentions_token("`PDEAD`", "PDEAD"));
    assert!(mentions_token("P1\\|P2", "P2"));
    assert!(!mentions_token("P10", "P1"));
    assert!(!mentions_token("P5PhantomDone", "P5"));
    assert!(!mentions_token("PDEADX", "PDEAD"));
}

/// Each surface reports a token no detector has, whether as missing or as
/// unresolvable, so the parity checks above can fail on every surface.
#[test]
fn an_unregistered_token_is_reported_on_every_surface() {
    let gaps = registration_gaps(&["PNOTAREALDETECTOR"], all_surfaces());
    assert_eq!(
        gaps.len(),
        all_surfaces().count(),
        "expected exactly one gap per surface; got:\n  {}",
        gaps.join("\n  ")
    );
}
