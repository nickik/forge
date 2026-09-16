from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
path = ROOT / "crates/forge-codegen-cranelift/src/function_c14_scalar.rs"
text = path.read_text()
old = '''    if !fir.closures.is_empty() {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
new = '''    if !fir.closures.is_empty()\n        || fir.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {\n            matches!(&instruction.kind, FirInstructionKind::CallClosure { .. })\n        })\n    {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
if text.count(old) != 1:
    raise SystemExit(f"expected scalar routing anchor once, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))
print("materialized closure-call routing")
