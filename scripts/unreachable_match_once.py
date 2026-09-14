from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement target in {path}")
    p.write_text(text.replace(old, new, 1))


path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()
old = '''        let mut covered = BTreeSet::new();
        for arm in arms {
            if arm.guard.is_some() {
                continue;
            }
            covered.extend(self.pattern_match_cases(&arm.pattern, ty, &required));
        }
        let missing = required.difference(&covered).cloned().collect::<Vec<_>>();
'''
new = '''        let mut covered = BTreeSet::new();
        for arm in arms {
            let arm_cases = self.pattern_match_cases(&arm.pattern, ty, &required);
            if !arm_cases.is_empty() && arm_cases.is_subset(&covered) {
                self.diagnostic(
                    arm.pattern.span,
                    "match/unreachable-arm",
                    "match arm is unreachable because earlier unguarded arms cover all of its cases",
                );
                continue;
            }
            if arm.guard.is_none() {
                covered.extend(arm_cases);
            }
        }
        let missing = required.difference(&covered).cloned().collect::<Vec<_>>();
'''
if old not in text:
    raise SystemExit("match coverage block not found")
path.write_text(text.replace(old, new, 1))


tests = Path("crates/forge-frontend/tests/typecheck.rs")
text = tests.read_text()
addition = r'''

#[test]
fn finite_matches_report_provably_unreachable_arms() {
    let duplicate = check(
        r#"
        module test.unreachable_enum_arm;
        enum Color { Red, Green }
        fn code(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
                Color::Red => 2,
                Color::Green => 3,
            };
        }
        "#,
    );
    assert!(
        has(&duplicate, "match/unreachable-arm"),
        "{:?}",
        duplicate.diagnostics
    );

    let after_wildcard = check(
        r#"
        module test.unreachable_after_wildcard;
        enum Color { Red, Green }
        fn code(color: Color) -> i32 {
            return match (color) {
                _ => 1,
                Color::Green => 2,
            };
        }
        "#,
    );
    assert!(
        has(&after_wildcard, "match/unreachable-arm"),
        "{:?}",
        after_wildcard.diagnostics
    );

    let guarded = check(
        r#"
        module test.guarded_arm_does_not_cover;
        enum Color { Red, Green }
        fn choose(color: Color, flag: bool) -> i32 {
            return match (color) {
                Color::Red when flag => 1,
                Color::Red => 2,
                Color::Green => 3,
            };
        }
        "#,
    );
    assert!(guarded.diagnostics.is_empty(), "{:?}", guarded.diagnostics);
}
'''
if "fn finite_matches_report_provably_unreachable_arms()" not in text:
    tests.write_text(text + addition)


fixture = Path("examples/conformance/negative/19-unreachable-match-arm.fg")
fixture.write_text('''module examples.conformance.negative.unreachable_match_arm;

enum Color { Red, Green }

fn color_code(color: Color) -> i32 {
    return match (color) {
        Color::Red => 1,
        Color::Red => 2,
        Color::Green => 3,
    };
}
''')


suite = Path("examples/conformance/suite.fdn")
text = suite.read_text()
needle = '    {:path #path "negative/18-non-exhaustive-match.fg" :kind :negative :expect :match/non-exhaustive}\n'
addition = needle + '    {:path #path "negative/19-unreachable-match-arm.fg" :kind :negative :expect :match/unreachable-arm}\n'
if 'negative/19-unreachable-match-arm.fg' not in text:
    if needle not in text:
        raise SystemExit("suite insertion point not found")
    suite.write_text(text.replace(needle, addition, 1))


arch = Path("docs/compiler-architecture.md")
text = arch.read_text()
old = '- Exhaustiveness for finite built-in/nominal sums (`bool`, optionals, enums, tagged unions) belongs in semantic type checking because this is the first layer with both resolved type identity and typed patterns. More advanced pattern-matrix optimization can remain a later pass.\n'
new = '- Exhaustiveness and provably unreachable arms for finite built-in/nominal sums (`bool`, optionals, enums, tagged unions) belong in semantic type checking because this is the first layer with both resolved type identity and typed patterns. The current check is deliberately conservative for guarded/refutable payload patterns; full pattern-matrix usefulness and decision-tree optimization remain a later pass.\n'
if old in text:
    arch.write_text(text.replace(old, new, 1))

spec = Path("docs/forge-v1-spec.md")
text = spec.read_text()
old = 'Metadata attaches to declarations and declaration-owned fields/methods. It is not an expression operator or a postfix type operator: forms such as `value @unchecked`, `u8 @range(...)`, and `@wrap(expr)` are not Forge v1 metadata syntax. A constrained alias instead carries `@range(...)` on the alias declaration itself.\n'
new = 'Metadata attaches to declarations and declaration-owned fields/methods. It is not an expression operator or a postfix type operator: forms such as `value @unchecked`, `u8 @range(...)`, and `@wrap(expr)` are not Forge v1 metadata syntax. Forge v1 does not standardize constrained/refined types; a metadata name such as `@range` may still be preserved as ordinary tool metadata without changing type semantics.\n'
if old not in text:
    raise SystemExit("stale constrained-alias wording not found")
spec.write_text(text.replace(old, new, 1))

print("unreachable match analysis patch applied")
