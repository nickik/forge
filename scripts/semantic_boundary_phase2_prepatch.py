from pathlib import Path

path = Path("scripts/semantic_boundary_phase2.py")
text = path.read_text()
old = r'''pattern = r''' + "'''" + r'''            HirExprKind::Closure \\{\\n                params,\\n                return_type,\\n                body,\\n                \\.\\.,\\n            \\} => \\{.*?\\n            \\}\\n            HirExprKind::Match \\{ value, arms \\} => \\{''' + "'''"
new = r'''pattern = r''' + "'''" + r'''            HirExprKind::Closure \\{.*?\\n            HirExprKind::Match \\{ value, arms \\} => \\{''' + "'''"
if old not in text:
    raise SystemExit("phase2 closure regex source not found")
path.write_text(text.replace(old, new, 1))
print("phase2 migration matcher normalized")
