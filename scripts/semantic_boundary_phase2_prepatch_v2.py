from pathlib import Path
import re

path = Path("scripts/semantic_boundary_phase2.py")
text = path.read_text()
pattern = r"pattern = r'''[\s\S]*?'''\nreplacement = r'''"
replacement = "pattern = r'''            HirExprKind::Closure \\\\{.*?\\n            HirExprKind::Match \\\\{ value, arms \\\\} => \\\\{'''\nreplacement = r'''"
text, count = re.subn(pattern, replacement, text, count=1)
if count != 1:
    raise SystemExit(f"could not normalize phase2 closure matcher: {count}")
path.write_text(text)
print("phase2 closure matcher normalized")
