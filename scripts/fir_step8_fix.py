from pathlib import Path

path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()
old = '#[serde(tag = "argument", rename_all = "snake_case")]\npub enum ResolvedCallArgument {'
new = '#[serde(tag = "source", rename_all = "snake_case")]\npub enum ResolvedCallArgument {'
if old not in text:
    raise SystemExit("ResolvedCallArgument serde tag anchor missing")
path.write_text(text.replace(old, new, 1))
