from pathlib import Path

p = Path("crates/forge-frontend/tests/fir.rs")
s = p.read_text()
old = '''#[test]
fn map_match_still_waits_for_collection_pattern_protocol() {
    let output = lower(
        r#"
        module test.fir_map_match_later;
        fn choose(values: u32[]) -> i32 {
            return match (values) { {:name ignored, ..} => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}

'''
if old not in s:
    raise SystemExit("obsolete map FIR test anchor missing")
s = s.replace(old, "", 1)
old2 = '''                {:age age?, ..} => {},
                _ => {},
'''
new2 = '''                {:name name, :age age?, ..} => {},
                _ => {},
'''
if old2 not in s:
    raise SystemExit("optional/rest map test anchor missing")
s = s.replace(old2, new2, 1)
p.write_text(s)
