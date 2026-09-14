from pathlib import Path

# The Step 6 migration adds sequence length/index lowering. Reuse one exact
# target-independent usize type helper instead of repeating pointer-width ints.
fir = Path("crates/forge-frontend/src/fir_v1.rs")
text = fir.read_text()
marker = "fn binary_overflow(op: BinaryOp, mode: OverflowMode) -> Option<OverflowMode> {\n"
helper = '''fn usize_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    }
}

'''
if marker not in text:
    raise SystemExit("FIR helper insertion point not found")
text = text.replace(marker, helper + marker, 1)
fir.write_text(text)

# A guarded Rust match arm for None only covers the Optional case semantically;
# make the non-Optional fallback explicit for Rust's exhaustiveness checker.
tc = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = tc.read_text()
old = '''            HirPatternKind::Or { patterns } => {
                let mut alternatives = Vec::new();
                for pattern in patterns {
                    alternatives.extend(self.plan_match_pattern_at(pattern, ty, projections)?);
                }
                Some(alternatives)
            }
            HirPatternKind::Map { .. } => None,
'''
new = '''            HirPatternKind::Or { patterns } => {
                let mut alternatives = Vec::new();
                for pattern in patterns {
                    alternatives.extend(self.plan_match_pattern_at(pattern, ty, projections)?);
                }
                Some(alternatives)
            }
            HirPatternKind::None { .. } => None,
            HirPatternKind::Map { .. } => None,
'''
if old not in text:
    raise SystemExit("None fallback insertion point not found")
tc.write_text(text.replace(old, new, 1))

Path("scripts/fir_step6_fix.py").unlink()
