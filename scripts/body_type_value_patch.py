from pathlib import Path

p = Path('crates/forge-frontend/src/body_hir_v1.rs')
s = p.read_text()
old = '''        if let Some(id) = self.module.symbols.get(name).and_then(|s| s.value_def) {
            return ResolvedName::Def(id);
        }
        if let Some(index) = self.imports.get(name).copied() {
'''
new = '''        if let Some(id) = self.module.symbols.get(name).and_then(|s| s.value_def) {
            return ResolvedName::Def(id);
        }
        // Preserve a type used in expression position. The type checker owns the
        // semantic diagnostic for illegal uses such as `value[Point]`.
        if let Some(id) = self.module.symbols.get(name).and_then(|s| s.type_def) {
            return ResolvedName::Def(id);
        }
        if is_builtin_type(name) {
            return ResolvedName::BuiltinType;
        }
        if let Some(index) = self.imports.get(name).copied() {
'''
assert old in s
s = s.replace(old, new, 1)
p.write_text(s)
