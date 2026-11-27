//! The one text form of an [`InstancePath`], both ways: `Display` prints a
//! path and `FromStr` parses back exactly the text `Display` prints, so the
//! two invert each other. The grammar is rule F of
//! docs/instance-kinds-design.md.

use std::fmt::{self, Write as _};
use std::str::FromStr;

use crate::instance_path::{
    AutoAnchor, AutoSite, AutoSiteSegment, CallChain, DefinitionName, InstancePath,
    InstancePathError, ModuleName, PathElement, PathKey, PathName, PathStep, is_name_char,
};

const MODULE_SEPARATOR: &str = "::";

/// The anchor word of a `connect`'s unknown, `connect[k]`.
const CONNECT: &str = "connect";

/// The word after a slash that names a fixed child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    Summary,
    AsValue,
    Defs,
    Constraint,
    Braces,
    Purpose,
    Arguments,
    Guard,
    Count,
    At,
    Auto,
    Made,
    Open,
    Problem,
    Solves,
}

impl Word {
    const ALL: [Word; 15] = [
        Word::Summary,
        Word::AsValue,
        Word::Defs,
        Word::Constraint,
        Word::Braces,
        Word::Purpose,
        Word::Arguments,
        Word::Guard,
        Word::Count,
        Word::At,
        Word::Auto,
        Word::Made,
        Word::Open,
        Word::Problem,
        Word::Solves,
    ];

    fn text(self) -> &'static str {
        match self {
            Word::Summary => "summary",
            Word::AsValue => "as-value",
            Word::Defs => "defs",
            Word::Constraint => "constraint",
            Word::Braces => "braces",
            Word::Purpose => "purpose",
            Word::Arguments => "arguments",
            Word::Guard => "guard",
            Word::Count => "count",
            Word::At => "at",
            Word::Auto => "auto",
            Word::Made => "made",
            Word::Open => "open",
            Word::Problem => "problem",
            Word::Solves => "solves",
        }
    }

    fn from_text(text: &str) -> Option<Word> {
        Word::ALL.into_iter().find(|word| word.text() == text)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_lowercase() || c == '-'
}

// ── printing ─────────────────────────────────────────────────────────────────

impl fmt::Display for InstancePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_definition(f, self.root())?;
        self.steps().iter().try_for_each(|step| write_step(f, step))
    }
}

fn write_definition(f: &mut fmt::Formatter<'_>, definition: &DefinitionName) -> fmt::Result {
    if let Some(module) = definition.module() {
        for (position, segment) in module.segments().iter().enumerate() {
            if position > 0 {
                f.write_char('.')?;
            }
            f.write_str(segment.as_str())?;
        }
        f.write_str(MODULE_SEPARATOR)?;
    }
    f.write_str(definition.name().as_str())
}

fn write_step(f: &mut fmt::Formatter<'_>, step: &PathStep) -> fmt::Result {
    match step {
        PathStep::Member(name) => write!(f, ".{}", name.as_str()),
        PathStep::Element(element) => write_element(f, element),
        PathStep::Summary => write_word(f, Word::Summary),
        PathStep::AsValue => write_word(f, Word::AsValue),
        PathStep::Defs(definition) => {
            write_word(f, Word::Defs)?;
            write_bracketed(f, |f| write_definition(f, definition))
        }
        PathStep::Constraint(k) => write_numbered(f, Word::Constraint, *k),
        PathStep::Braces => write_word(f, Word::Braces),
        PathStep::Purpose(k) => write_numbered(f, Word::Purpose, *k),
        PathStep::Arguments => write_word(f, Word::Arguments),
        PathStep::Guard(k) => write_numbered(f, Word::Guard, *k),
        PathStep::Count(name) => write_named(f, Word::Count, name),
        PathStep::At(name) => write_named(f, Word::At, name),
        PathStep::Auto(site) => {
            write_word(f, Word::Auto)?;
            write_bracketed(f, |f| write_site(f, site))
        }
        PathStep::MadeList => write_word(f, Word::Made),
        PathStep::Made(chain) => write_chained(f, Word::Made, chain),
        PathStep::Open(None) => write_word(f, Word::Open),
        PathStep::Open(Some(chain)) => write_chained(f, Word::Open, chain),
        PathStep::Problem => write_word(f, Word::Problem),
        PathStep::Solves => write_word(f, Word::Solves),
    }
}

fn write_word(f: &mut fmt::Formatter<'_>, word: Word) -> fmt::Result {
    write!(f, "/{}", word.text())
}

fn write_bracketed(
    f: &mut fmt::Formatter<'_>,
    inner: impl FnOnce(&mut fmt::Formatter<'_>) -> fmt::Result,
) -> fmt::Result {
    f.write_char('[')?;
    inner(f)?;
    f.write_char(']')
}

fn write_numbered(f: &mut fmt::Formatter<'_>, word: Word, k: u64) -> fmt::Result {
    write_word(f, word)?;
    write!(f, "[{k}]")
}

fn write_named(f: &mut fmt::Formatter<'_>, word: Word, name: &PathName) -> fmt::Result {
    write_word(f, word)?;
    write!(f, "[{}]", name.as_str())
}

fn write_chained(f: &mut fmt::Formatter<'_>, word: Word, chain: &CallChain) -> fmt::Result {
    write_word(f, word)?;
    write_bracketed(f, |f| write_chain(f, chain))
}

fn write_element(f: &mut fmt::Formatter<'_>, element: &PathElement) -> fmt::Result {
    write_bracketed(f, |f| match element {
        PathElement::Index(i) => write!(f, "{i}"),
        PathElement::Key(key) => write_key(f, key),
    })
}

fn write_key(f: &mut fmt::Formatter<'_>, key: &PathKey) -> fmt::Result {
    f.write_char('"')?;
    for c in key.as_str().chars() {
        if matches!(c, '"' | '\\') {
            f.write_char('\\')?;
        }
        f.write_char(c)?;
    }
    f.write_char('"')
}

fn write_chain(f: &mut fmt::Formatter<'_>, chain: &CallChain) -> fmt::Result {
    for (position, index) in chain.indices().iter().enumerate() {
        if position > 0 {
            f.write_char(',')?;
        }
        write!(f, "{index}")?;
    }
    Ok(())
}

fn write_site(f: &mut fmt::Formatter<'_>, site: &AutoSite) -> fmt::Result {
    match site.anchor() {
        AutoAnchor::Member { name, key } => {
            f.write_str(name.as_str())?;
            if let Some(key) = key {
                write_bracketed(f, |f| write_key(f, key))?;
            }
        }
        AutoAnchor::Connect(k) => write!(f, "{CONNECT}[{k}]")?,
    }
    site.segments()
        .iter()
        .try_for_each(|segment| match segment {
            AutoSiteSegment::Argument(name) => write!(f, ".{}", name.as_str()),
            AutoSiteSegment::Chain(chain) => {
                f.write_char('#')?;
                write_chain(f, chain)
            }
            AutoSiteSegment::Placement => write_word(f, Word::At),
        })
}

// ── parsing ──────────────────────────────────────────────────────────────────

impl FromStr for InstancePath {
    type Err = PathTextError;

    fn from_str(text: &str) -> Result<Self, PathTextError> {
        if text.is_empty() {
            return Err(PathTextError::at(0, PathTextProblem::Empty));
        }
        let mut parser = Parser { text, at: 0 };
        let (root, members) = parser.definition_and_members()?;
        let path =
            InstancePath::from_steps(root, members.into_iter().map(PathStep::Member).collect())
                .map_err(|e| PathTextError::at(0, PathTextProblem::Invalid(e)))?;
        parser.steps(path)
    }
}

type Parsed<T> = Result<T, PathTextError>;

struct Parser<'t> {
    text: &'t str,
    at: usize,
}

impl<'t> Parser<'t> {
    fn rest(&self) -> &'t str {
        &self.text[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, token: &str) -> bool {
        let found = self.rest().starts_with(token);
        if found {
            self.at += token.len();
        }
        found
    }

    fn expect(&mut self, token: &'static str) -> Parsed<()> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.problem(PathTextProblem::Expected(token)))
        }
    }

    fn problem(&self, problem: PathTextProblem) -> PathTextError {
        PathTextError::at(self.at, problem)
    }

    /// The longest run of characters from here that `keep` accepts.
    fn run(&mut self, keep: impl Fn(char) -> bool) -> &'t str {
        let rest = self.rest();
        let len = rest.find(|c: char| !keep(c)).unwrap_or(rest.len());
        self.at += len;
        &rest[..len]
    }

    fn bracketed<T>(&mut self, inner: impl FnOnce(&mut Self) -> Parsed<T>) -> Parsed<T> {
        self.expect("[")?;
        let value = inner(self)?;
        self.expect("]")?;
        Ok(value)
    }

    fn name(&mut self) -> Parsed<PathName> {
        let start = self.at;
        let run = self.run(is_name_char);
        if run.is_empty() {
            return Err(PathTextError::at(
                start,
                PathTextProblem::ExpectedIdentifier,
            ));
        }
        PathName::new(run).map_err(|e| PathTextError::at(start, PathTextProblem::Invalid(e)))
    }

    fn number(&mut self) -> Parsed<u64> {
        let start = self.at;
        let digits = self.run(|c| c.is_ascii_digit());
        let refused = |problem| Err(PathTextError::at(start, problem));
        if digits.is_empty() {
            return refused(PathTextProblem::ExpectedNumber);
        }
        if digits.len() > 1 && digits.starts_with('0') {
            return refused(PathTextProblem::NonCanonicalNumber);
        }
        digits
            .parse()
            .or_else(|_| refused(PathTextProblem::NumberOutOfRange))
    }

    fn chain(&mut self) -> Parsed<CallChain> {
        let start = self.at;
        let mut indices = vec![self.number()?];
        while self.eat(",") {
            indices.push(self.number()?);
        }
        CallChain::new(indices).map_err(|e| PathTextError::at(start, PathTextProblem::Invalid(e)))
    }

    fn key(&mut self) -> Parsed<PathKey> {
        let open = self.at;
        self.expect("\"")?;
        let unterminated = || PathTextError::at(open, PathTextProblem::UnterminatedKey);
        let mut key = String::new();
        loop {
            let at = self.at;
            match self.next_char().ok_or_else(unterminated)? {
                '"' => return Ok(PathKey::new(key)),
                '\\' => match self.next_char().ok_or_else(unterminated)? {
                    escaped @ ('"' | '\\') => key.push(escaped),
                    other => {
                        return Err(PathTextError::at(at, PathTextProblem::UnknownEscape(other)));
                    }
                },
                c => key.push(c),
            }
        }
    }

    /// A definition's dotted run: a module and a definition when `::`
    /// follows it, else a definition and the members after it.
    fn definition_and_members(&mut self) -> Parsed<(DefinitionName, Vec<PathName>)> {
        let start = self.at;
        let first = self.name()?;
        let mut rest = Vec::new();
        while self.eat(".") {
            rest.push(self.name()?);
        }
        if !self.eat(MODULE_SEPARATOR) {
            return Ok((DefinitionName::new(None, first), rest));
        }
        let segments = std::iter::once(first).chain(rest).collect();
        let module = ModuleName::new(segments)
            .map_err(|e| PathTextError::at(start, PathTextProblem::Invalid(e)))?;
        let definition = DefinitionName::new(Some(module), self.name()?);
        Ok((definition, Vec::new()))
    }

    fn definition(&mut self) -> Parsed<DefinitionName> {
        let start = self.at;
        let (definition, members) = self.definition_and_members()?;
        if members.is_empty() {
            Ok(definition)
        } else {
            let first_dot = start + definition.name().as_str().len();
            Err(PathTextError::at(first_dot, PathTextProblem::Expected("]")))
        }
    }

    fn steps(&mut self, mut path: InstancePath) -> Parsed<InstancePath> {
        loop {
            let start = self.at;
            let step = match self.peek() {
                None => return Ok(path),
                Some('.') => {
                    self.at += 1;
                    PathStep::Member(self.name()?)
                }
                Some('[') => PathStep::Element(self.element()?),
                Some('/') => {
                    self.at += 1;
                    self.fixed_child(start)?
                }
                Some(_) => return Err(self.problem(PathTextProblem::TrailingText)),
            };
            path = path
                .then(step)
                .map_err(|e| PathTextError::at(start, PathTextProblem::Invalid(e)))?;
        }
    }

    fn element(&mut self) -> Parsed<PathElement> {
        self.bracketed(|parser| match parser.peek() {
            Some('"') => Ok(PathElement::Key(parser.key()?)),
            Some(c) if c.is_ascii_digit() => Ok(PathElement::Index(parser.number()?)),
            _ => Err(parser.problem(PathTextProblem::ExpectedIndexOrKey)),
        })
    }

    fn word(&mut self) -> Parsed<Word> {
        let start = self.at;
        let run = self.run(is_word_char);
        Word::from_text(run)
            .ok_or_else(|| PathTextError::at(start, PathTextProblem::UnknownStepWord(run.into())))
    }

    fn fixed_child(&mut self, step_start: usize) -> Parsed<PathStep> {
        let bracket_follows = |parser: &Self| parser.peek() == Some('[');
        Ok(match self.word()? {
            Word::Summary => PathStep::Summary,
            Word::AsValue => PathStep::AsValue,
            Word::Defs => PathStep::Defs(self.bracketed(Self::definition)?),
            Word::Constraint => PathStep::Constraint(self.bracketed(Self::number)?),
            Word::Braces => PathStep::Braces,
            Word::Purpose => PathStep::Purpose(self.bracketed(Self::number)?),
            Word::Arguments => PathStep::Arguments,
            Word::Guard => PathStep::Guard(self.bracketed(Self::number)?),
            Word::Count => PathStep::Count(self.bracketed(Self::name)?),
            Word::At => PathStep::At(self.bracketed(Self::name)?),
            Word::Auto => PathStep::Auto(self.bracketed(|parser| parser.site(step_start))?),
            Word::Made if bracket_follows(self) => PathStep::Made(self.bracketed(Self::chain)?),
            Word::Made => PathStep::MadeList,
            Word::Open if bracket_follows(self) => {
                PathStep::Open(Some(self.bracketed(Self::chain)?))
            }
            Word::Open => PathStep::Open(None),
            Word::Problem => PathStep::Problem,
            Word::Solves => PathStep::Solves,
        })
    }

    fn site(&mut self, step_start: usize) -> Parsed<AutoSite> {
        let anchor = self.anchor()?;
        let mut segments = Vec::new();
        loop {
            let segment = if self.eat(".") {
                AutoSiteSegment::Argument(self.name()?)
            } else if self.eat("#") {
                AutoSiteSegment::Chain(self.chain()?)
            } else if self.eat("/") {
                self.placement_word()?;
                AutoSiteSegment::Placement
            } else {
                break;
            };
            segments.push(segment);
        }
        AutoSite::new(anchor, segments)
            .map_err(|e| PathTextError::at(step_start, PathTextProblem::Invalid(e)))
    }

    fn placement_word(&mut self) -> Parsed<()> {
        let start = self.at;
        if self.run(is_word_char) == Word::At.text() {
            Ok(())
        } else {
            Err(PathTextError::at(
                start,
                PathTextProblem::Expected(Word::At.text()),
            ))
        }
    }

    /// A member, with a quoted key for a keyed entry, or `connect[k]`.
    fn anchor(&mut self) -> Parsed<AutoAnchor> {
        let name = self.name()?;
        if self.peek() != Some('[') {
            return Ok(AutoAnchor::Member { name, key: None });
        }
        self.bracketed(|parser| {
            let digit_follows = parser.peek().is_some_and(|c| c.is_ascii_digit());
            if name.as_str() == CONNECT && digit_follows {
                Ok(AutoAnchor::Connect(parser.number()?))
            } else {
                let key = Some(parser.key()?);
                Ok(AutoAnchor::Member { name, key })
            }
        })
    }
}

/// Why a text is not a path's text, and the byte where it goes wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTextError {
    offset: usize,
    problem: PathTextProblem,
}

impl PathTextError {
    fn at(offset: usize, problem: PathTextProblem) -> Self {
        Self { offset, problem }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn problem(&self) -> &PathTextProblem {
        &self.problem
    }
}

impl fmt::Display for PathTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "at byte {} of the path text: {}",
            self.offset, self.problem
        )
    }
}

impl std::error::Error for PathTextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.problem {
            PathTextProblem::Invalid(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathTextProblem {
    Empty,
    ExpectedIdentifier,
    ExpectedNumber,
    ExpectedIndexOrKey,
    NonCanonicalNumber,
    NumberOutOfRange,
    Expected(&'static str),
    UnknownStepWord(String),
    UnknownEscape(char),
    UnterminatedKey,
    TrailingText,
    /// The text names parts a path cannot be built from.
    Invalid(InstancePathError),
}

impl fmt::Display for PathTextProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the text is empty"),
            Self::ExpectedIdentifier => write!(f, "expected a name"),
            Self::ExpectedNumber => write!(f, "expected a number"),
            Self::ExpectedIndexOrKey => write!(f, "expected an index or a quoted key"),
            Self::NonCanonicalNumber => write!(f, "a number is written without leading zeros"),
            Self::NumberOutOfRange => write!(f, "the number does not fit in 64 bits"),
            Self::Expected(token) => write!(f, "expected `{token}`"),
            Self::UnknownStepWord(word) => write!(f, "`{word}` is not a kind of step"),
            Self::UnknownEscape(c) => write!(
                f,
                "`\\{c}` is not an escape: a key escapes only a quote and a backslash"
            ),
            Self::UnterminatedKey => write!(f, "the key that opens here has no closing quote"),
            Self::TrailingText => write!(f, "expected a step or the end of the path"),
            Self::Invalid(e) => write!(f, "{e}"),
        }
    }
}
