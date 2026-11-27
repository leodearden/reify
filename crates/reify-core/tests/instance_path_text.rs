//! The one text form of an instance path, through `Display` and `FromStr`
//! only: every example text names the path it was written for, both ways
//! (scenario H1 of docs/prds/instances-on-the-core.md), generated paths
//! print to distinct texts that parse back to them, and text that is not
//! canonical is refused at a byte offset.

use std::collections::HashMap;

use reify_core::{
    AutoAnchor, AutoSite, AutoSiteSegment, CallChain, DefinitionName, InstancePath,
    InstancePathError, ModuleName, PathElement, PathKey, PathName, PathStep, PathTextError,
    PathTextProblem,
};

fn n(text: &str) -> PathName {
    PathName::new(text).unwrap_or_else(|e| panic!("`{text}` should be a name: {e}"))
}

fn entry(definition: &str) -> DefinitionName {
    DefinitionName::new(None, n(definition))
}

fn in_module(segments: &[&str], definition: &str) -> DefinitionName {
    let module = ModuleName::new(segments.iter().map(|s| n(s)).collect()).expect("segments");
    DefinitionName::new(Some(module), n(definition))
}

fn path(root: DefinitionName, steps: Vec<PathStep>) -> InstancePath {
    InstancePath::from_steps(root, steps).expect("a valid path")
}

fn member(name: &str) -> PathStep {
    PathStep::Member(n(name))
}

fn index(i: u64) -> PathStep {
    PathStep::Element(PathElement::Index(i))
}

fn keyed(key: &str) -> PathStep {
    PathStep::Element(PathElement::Key(PathKey::new(key)))
}

fn chain(indices: &[u64]) -> CallChain {
    CallChain::new(indices.to_vec()).expect("a non-empty chain")
}

fn on(name: &str) -> AutoAnchor {
    AutoAnchor::Member {
        name: n(name),
        key: None,
    }
}

fn arg(name: &str) -> AutoSiteSegment {
    AutoSiteSegment::Argument(n(name))
}

fn auto(anchor: AutoAnchor, segments: Vec<AutoSiteSegment>) -> PathStep {
    PathStep::Auto(AutoSite::new(anchor, segments).expect("a valid site"))
}

#[test]
fn scenario_h1() {
    let asm = || entry("Asm");
    let rows: Vec<(&str, InstancePath)> = vec![
        ("Asm", path(asm(), vec![])),
        ("Asm.thin.t", path(asm(), vec![member("thin"), member("t")])),
        ("Asm.vents[2]", path(asm(), vec![member("vents"), index(2)])),
        (
            r#"Asm.vents["intake"]"#,
            path(asm(), vec![member("vents"), keyed("intake")]),
        ),
        ("Asm/summary", path(asm(), vec![PathStep::Summary])),
        (
            "Asm.b/summary",
            path(asm(), vec![member("b"), PathStep::Summary]),
        ),
        (
            "Asm.a/as-value",
            path(asm(), vec![member("a"), PathStep::AsValue]),
        ),
        (
            "Asm/defs[Plate]",
            path(asm(), vec![PathStep::Defs(entry("Plate"))]),
        ),
        (
            "Asm.b/constraint[0]",
            path(asm(), vec![member("b"), PathStep::Constraint(0)]),
        ),
        (
            "Asm/constraint[1][3]",
            path(asm(), vec![PathStep::Constraint(1), index(3)]),
        ),
        (
            r#"Manifold/constraint[0]["intake"]"#,
            path(
                entry("Manifold"),
                vec![PathStep::Constraint(0), keyed("intake")],
            ),
        ),
        (
            "Rig.motor/braces/constraint[0]",
            path(
                entry("Rig"),
                vec![member("motor"), PathStep::Braces, PathStep::Constraint(0)],
            ),
        ),
        (
            "Asm.s/braces.zz",
            path(asm(), vec![member("s"), PathStep::Braces, member("zz")]),
        ),
        (
            "Asm.p/braces/guard[0]",
            path(
                asm(),
                vec![member("p"), PathStep::Braces, PathStep::Guard(0)],
            ),
        ),
        (
            "Asm/purpose[0]/constraint[0]",
            path(asm(), vec![PathStep::Purpose(0), PathStep::Constraint(0)]),
        ),
        (
            "Asm.b/arguments",
            path(asm(), vec![member("b"), PathStep::Arguments]),
        ),
        (
            "Case/guard[0]",
            path(entry("Case"), vec![PathStep::Guard(0)]),
        ),
        (
            "Asm/count[vents]",
            path(asm(), vec![PathStep::Count(n("vents"))]),
        ),
        ("Asm/at[a]", path(asm(), vec![PathStep::At(n("a"))])),
        (
            "Asm/auto[a.len]",
            path(asm(), vec![auto(on("a"), vec![arg("len")])]),
        ),
        (
            r#"M/auto[vents["a"].w]"#,
            path(
                entry("M"),
                vec![auto(
                    AutoAnchor::Member {
                        name: n("vents"),
                        key: Some(PathKey::new("a")),
                    },
                    vec![arg("w")],
                )],
            ),
        ),
        (
            "Asm/auto[both#1.t]",
            path(
                asm(),
                vec![auto(
                    on("both"),
                    vec![AutoSiteSegment::Chain(chain(&[1])), arg("t")],
                )],
            ),
        ),
        (
            "Asm/auto[d.p#0.t]",
            path(
                asm(),
                vec![auto(
                    on("d"),
                    vec![arg("p"), AutoSiteSegment::Chain(chain(&[0])), arg("t")],
                )],
            ),
        ),
        (
            "Asm.c/made[0]",
            path(asm(), vec![member("c"), PathStep::Made(chain(&[0]))]),
        ),
        (
            "Asm.c/made[0,2]",
            path(asm(), vec![member("c"), PathStep::Made(chain(&[0, 2]))]),
        ),
        (
            "Asm.c/made",
            path(asm(), vec![member("c"), PathStep::MadeList]),
        ),
        (
            "Asm.c/open.area",
            path(
                asm(),
                vec![member("c"), PathStep::Open(None), member("area")],
            ),
        ),
        (
            "Asm.m/open[0].area",
            path(
                asm(),
                vec![
                    member("m"),
                    PathStep::Open(Some(chain(&[0]))),
                    member("area"),
                ],
            ),
        ),
        (
            "Asm.a/problem",
            path(asm(), vec![member("a"), PathStep::Problem]),
        ),
        (
            "Asm.a/solves",
            path(asm(), vec![member("a"), PathStep::Solves]),
        ),
        (
            "Bracket/purpose[0]/constraint[0]/made[0]/constraint[0]",
            path(
                entry("Bracket"),
                vec![
                    PathStep::Purpose(0),
                    PathStep::Constraint(0),
                    PathStep::Made(chain(&[0])),
                    PathStep::Constraint(0),
                ],
            ),
        ),
        (
            "lib::Motor.d",
            path(in_module(&["lib"], "Motor"), vec![member("d")]),
        ),
        (
            "std.units::Plate/summary",
            path(
                in_module(&["std", "units"], "Plate"),
                vec![PathStep::Summary],
            ),
        ),
        (
            "Asm/defs[lib::Motor]",
            path(asm(), vec![PathStep::Defs(in_module(&["lib"], "Motor"))]),
        ),
        (
            "Asm/auto[connect[0].p]",
            path(asm(), vec![auto(AutoAnchor::Connect(0), vec![arg("p")])]),
        ),
        (
            "Asm/auto[a/at]",
            path(asm(), vec![auto(on("a"), vec![AutoSiteSegment::Placement])]),
        ),
        (
            r#"Asm.ducts["a.b/c]"].area"#,
            path(
                asm(),
                vec![member("ducts"), keyed("a.b/c]"), member("area")],
            ),
        ),
        (
            r#"Asm.k["say \"hi\" \\ bye"]"#,
            path(asm(), vec![member("k"), keyed(r#"say "hi" \ bye"#)]),
        ),
        (
            "Asm.summary.area",
            path(asm(), vec![member("summary"), member("area")]),
        ),
        (
            "Asm.braces.x",
            path(asm(), vec![member("braces"), member("x")]),
        ),
        (
            "Asm/braces.x",
            path(asm(), vec![PathStep::Braces, member("x")]),
        ),
    ];
    for (text, built) in rows {
        assert_eq!(text.parse::<InstancePath>().as_ref(), Ok(&built), "{text}");
        assert_eq!(built.to_string(), text);
    }
}

// ── generated round trip ─────────────────────────────────────────────────────

const GENERATED_PATHS: usize = 5000;
const MAX_STEPS: usize = 8;

/// The fixed-child words, so a member named like one is drawn often.
const FIXED_WORDS: &[&str] = &[
    "summary",
    "braces",
    "made",
    "open",
    "auto",
    "at",
    "connect",
    "constraint",
    "defs",
    "solves",
    "problem",
    "arguments",
    "guard",
    "count",
    "purpose",
];

/// Fragments a key is joined from: every character the text gives a meaning
/// to, the module separator, and non-ASCII text.
const KEY_FRAGMENTS: &[&str] = &[
    ".", "/", "[", "]", "\"", "\\", "#", ",", "::", "é", "ключ", " ", "intake",
];

/// xorshift64, fixed seed: the same paths on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

type Draw<T> = fn(&mut Rng) -> Option<T>;

/// Declares one drawer per variant of a type and `kind_of`, the row a built
/// value matches. Its match is exhaustive, so a variant with no row, and so
/// never drawn, does not compile.
macro_rules! choices {
    ($draws:ident, $kind_of:ident, $ty:ty { $($pattern:pat => $draw:expr,)+ }) => {
        const $draws: &[Draw<$ty>] = &[$($draw,)+];

        fn $kind_of(value: &$ty) -> usize {
            match value {
                $($pattern => {})+
            }
            [$(matches!(value, $pattern),)+]
                .iter()
                .position(|&matched| matched)
                .expect("the match above is exhaustive")
        }
    };
}

choices!(STEPS, step_kind, PathStep {
    PathStep::Member(_) => |rng| Some(PathStep::Member(draw_name(rng))),
    PathStep::Element(_) => |rng| Some(PathStep::Element(draw(rng, ELEMENTS)?)),
    PathStep::Summary => |_| Some(PathStep::Summary),
    PathStep::AsValue => |_| Some(PathStep::AsValue),
    PathStep::Defs(_) => |rng| Some(PathStep::Defs(draw_definition(rng))),
    PathStep::Constraint(_) => |rng| Some(PathStep::Constraint(draw_number(rng))),
    PathStep::Braces => |_| Some(PathStep::Braces),
    PathStep::Purpose(_) => |rng| Some(PathStep::Purpose(draw_number(rng))),
    PathStep::Arguments => |_| Some(PathStep::Arguments),
    PathStep::Guard(_) => |rng| Some(PathStep::Guard(draw_number(rng))),
    PathStep::Count(_) => |rng| Some(PathStep::Count(draw_name(rng))),
    PathStep::At(_) => |rng| Some(PathStep::At(draw_name(rng))),
    PathStep::Auto(_) => |rng| Some(PathStep::Auto(draw_site(rng)?)),
    PathStep::MadeList => |_| Some(PathStep::MadeList),
    PathStep::Made(_) => |rng| Some(PathStep::Made(draw_chain(rng))),
    PathStep::Open(None) => |_| Some(PathStep::Open(None)),
    PathStep::Open(Some(_)) => |rng| Some(PathStep::Open(Some(draw_chain(rng)))),
    PathStep::Problem => |_| Some(PathStep::Problem),
    PathStep::Solves => |_| Some(PathStep::Solves),
});

choices!(ELEMENTS, element_kind, PathElement {
    PathElement::Index(_) => |rng| Some(PathElement::Index(draw_number(rng))),
    PathElement::Key(_) => |rng| Some(PathElement::Key(draw_key(rng))),
});

choices!(ANCHORS, anchor_kind, AutoAnchor {
    AutoAnchor::Member { .. } => |rng| {
        let name = draw_name(rng);
        let key = (rng.below(2) == 0).then(|| draw_key(rng));
        Some(AutoAnchor::Member { name, key })
    },
    AutoAnchor::Connect(_) => |rng| Some(AutoAnchor::Connect(draw_number(rng))),
});

choices!(SEGMENTS, segment_kind, AutoSiteSegment {
    AutoSiteSegment::Argument(_) => |rng| Some(AutoSiteSegment::Argument(draw_name(rng))),
    AutoSiteSegment::Chain(_) => |rng| Some(AutoSiteSegment::Chain(draw_chain(rng))),
    AutoSiteSegment::Placement => |_| Some(AutoSiteSegment::Placement),
});

fn draw<T>(rng: &mut Rng, draws: &[Draw<T>]) -> Option<T> {
    draws[rng.below(draws.len())](rng)
}

/// How often each choice appears in the kept paths, counted from the paths
/// themselves.
#[derive(Default)]
struct Coverage {
    roots: [u32; 2],
    steps: [u32; STEPS.len()],
    elements: [u32; ELEMENTS.len()],
    anchors: [u32; ANCHORS.len()],
    segments: [u32; SEGMENTS.len()],
}

impl Coverage {
    fn record(&mut self, path: &InstancePath) {
        self.roots[usize::from(path.root().module().is_some())] += 1;
        for step in path.steps() {
            self.steps[step_kind(step)] += 1;
            match step {
                PathStep::Element(element) => self.elements[element_kind(element)] += 1,
                PathStep::Auto(site) => {
                    self.anchors[anchor_kind(site.anchor())] += 1;
                    for segment in site.segments() {
                        self.segments[segment_kind(segment)] += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn assert_every_choice_drawn(&self) {
        let groups: [(&str, &[u32]); 5] = [
            ("root kind", &self.roots),
            ("step kind", &self.steps),
            ("element kind", &self.elements),
            ("anchor kind", &self.anchors),
            ("segment kind", &self.segments),
        ];
        for (group, counts) in groups {
            for (choice, count) in counts.iter().enumerate() {
                assert!(
                    *count > 0,
                    "{group} {choice} was never drawn into a kept path"
                );
            }
        }
    }
}

fn draw_name(rng: &mut Rng) -> PathName {
    if rng.below(2) == 0 {
        return n(rng.pick(FIXED_WORDS));
    }
    const START: &[u8] = b"abcXYZ_";
    const REST: &[u8] = b"az09_Z";
    let mut text = String::new();
    text.push(START[rng.below(START.len())] as char);
    for _ in 0..rng.below(5) {
        text.push(REST[rng.below(REST.len())] as char);
    }
    n(&text)
}

fn draw_number(rng: &mut Rng) -> u64 {
    match rng.below(6) {
        0 => 0,
        1 => 1,
        2 => 10,
        3 => u64::MAX,
        4 => rng.next(),
        _ => rng.below(100) as u64,
    }
}

fn draw_key(rng: &mut Rng) -> PathKey {
    let fragments: Vec<&str> = (0..rng.below(4)).map(|_| rng.pick(KEY_FRAGMENTS)).collect();
    PathKey::new(fragments.concat())
}

fn draw_chain(rng: &mut Rng) -> CallChain {
    let indices = (0..=rng.below(3)).map(|_| draw_number(rng)).collect();
    CallChain::new(indices).expect("at least one index")
}

fn draw_definition(rng: &mut Rng) -> DefinitionName {
    if rng.below(2) == 0 {
        return DefinitionName::new(None, draw_name(rng));
    }
    let segments = (0..=rng.below(3)).map(|_| draw_name(rng)).collect();
    let module = ModuleName::new(segments).expect("at least one segment");
    DefinitionName::new(Some(module), draw_name(rng))
}

fn draw_site(rng: &mut Rng) -> Option<AutoSite> {
    let anchor = draw(rng, ANCHORS)?;
    let segments = (0..=rng.below(4))
        .map(|_| draw(rng, SEGMENTS))
        .collect::<Option<Vec<_>>>()?;
    AutoSite::new(anchor, segments).ok()
}

fn draw_path(rng: &mut Rng) -> InstancePath {
    let mut path = InstancePath::new(draw_definition(rng));
    for _ in 0..rng.below(MAX_STEPS + 1) {
        let Some(step) = draw(rng, STEPS) else {
            continue;
        };
        if let Ok(longer) = path.then(step) {
            path = longer;
        }
    }
    path
}

#[test]
fn generated_paths_print_to_distinct_texts_that_parse_back() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut coverage = Coverage::default();
    let mut by_text: HashMap<String, InstancePath> = HashMap::new();
    for _ in 0..GENERATED_PATHS {
        let path = draw_path(&mut rng);
        coverage.record(&path);
        let text = path.to_string();
        let parsed = text
            .parse::<InstancePath>()
            .unwrap_or_else(|e| panic!("`{text}` from {path:?} does not parse: {e}"));
        assert_eq!(parsed, path, "{text}");
        if let Some(earlier) = by_text.insert(text.clone(), path.clone()) {
            assert_eq!(earlier, path, "two paths print as `{text}`");
        }
    }
    coverage.assert_every_choice_drawn();
}

// ── refusals ─────────────────────────────────────────────────────────────────

fn refusal(text: &str) -> PathTextError {
    let Err(err) = text.parse::<InstancePath>() else {
        panic!("`{text}` should be refused");
    };
    let message = err.to_string();
    assert!(
        message.starts_with(&format!("at byte {} of the path text: ", err.offset())),
        "{message}"
    );
    err
}

#[test]
fn text_that_is_not_canonical_is_refused_where_it_goes_wrong() {
    let rows: &[(&str, usize)] = &[
        ("", 0),
        (" Asm", 0),
        ("Asm ", 3),
        ("1Asm", 0),
        ("Asm.", 4),
        ("Asm..t", 4),
        ("Asm/", 4),
        ("Asm/sumary", 4),
        ("Asm/summary[", 12),
        ("Asm[01]", 4),
        ("Asm[-1]", 4),
        ("Asm[+1]", 4),
        ("Asm[1", 5),
        ("Asm[18446744073709551616]", 4),
        (r#"Asm["a]"#, 4),
        (r#"Asm["a\n"]"#, 6),
        ("Asm/constraint", 14),
        ("Asm/constraint[]", 15),
        ("Asm/made[]", 9),
        ("Asm/made[0,]", 11),
        ("Asm/made[,0]", 9),
        ("Asm/made[0, 1]", 11),
        ("Asm/auto[a]", 3),
        ("Asm/auto[a#0]", 3),
        ("Asm/auto[a#0#1.t]", 3),
        ("Asm/auto[vents[0].w]", 15),
        ("Asm/defs[a.b]", 10),
        ("::Asm", 0),
        ("lib::", 5),
        ("a::b::C", 4),
        ("lib..x::Asm", 4),
    ];
    for (text, offset) in rows {
        assert_eq!(refusal(text).offset(), *offset, "`{text}`");
    }
}

#[test]
fn a_refusal_says_what_went_wrong() {
    assert_eq!(refusal("").problem(), &PathTextProblem::Empty);
    assert_eq!(
        refusal("Asm/sumary").problem(),
        &PathTextProblem::UnknownStepWord("sumary".to_string())
    );
    assert_eq!(
        refusal(r#"Asm["a\n"]"#).problem(),
        &PathTextProblem::UnknownEscape('n')
    );
    assert_eq!(
        refusal("Asm[18446744073709551616]").problem(),
        &PathTextProblem::NumberOutOfRange
    );
    assert_eq!(
        refusal("Asm[01]").problem(),
        &PathTextProblem::NonCanonicalNumber
    );
    assert_eq!(
        refusal("Asm/auto[a]").problem(),
        &PathTextProblem::Invalid(InstancePathError::EmptyAutoSite)
    );
}
