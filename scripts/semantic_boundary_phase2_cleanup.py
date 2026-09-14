from pathlib import Path

p = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = p.read_text()
text = text.replace('self.env.lookup_method(&channel_ty, "recv")', 'self.env.lookup_method(&channel_ty, "receive")')
text = text.replace('select channel `recv` method must take only its receiver', 'select channel `receive` method must take only its receiver')
text = text.replace('does not provide the required `recv` method', 'does not provide the required `receive` method')
old = '''struct ParamSig {
    name: String,
    ty: Ty,
    has_default: bool,
    local: Option<LocalId>,
    default: Option<HirExpr>,
}
'''
new = '''struct ParamSig {
    name: String,
    ty: Ty,
    default: Option<HirExpr>,
}
'''
if old not in text:
    raise SystemExit("ParamSig cleanup shape missing")
text = text.replace(old, new, 1)
text = text.replace('''                            has_default: default.is_some(),
                            local,
                            default,
''', '''                            default,
''')
if 'struct ParamSig {\n    name: String,\n    ty: Ty,\n    has_default:' in text:
    raise SystemExit("stale ParamSig has_default field remains")
p.write_text(text)

p = Path("crates/forge-frontend/tests/typecheck.rs")
text = p.read_text().replace('fn recv(self: &Jobs) -> u32', 'fn receive(self: &Jobs) -> u32')
p.write_text(text)
print("semantic phase 2 cleanup applied")
