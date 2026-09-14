from pathlib import Path

p = Path("crates/forge-frontend/src/fir_v1.rs")
text = p.read_text()
old = '''                    let Some(value) =
                        self.lower_match_condition(span, scrutinee, scrutinee_ty, condition)
                    else {
                        return None;
                    };
'''
new = '''                    let value =
                        self.lower_match_condition(span, scrutinee, scrutinee_ty, condition)?;
'''
if old not in text:
    raise SystemExit("step 5 clippy target not found")
p.write_text(text.replace(old, new, 1))
Path("scripts/fir_step5_clippy_fix.py").unlink()
