from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Ordinary scalar lowering must hand functions containing CallClosure to the
# closure-aware path even when they define no local closures themselves.
path = ROOT / "crates/forge-codegen-cranelift/src/function_c14_scalar.rs"
text = path.read_text()
old = '''    if !fir.closures.is_empty() {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
new = '''    if !fir.closures.is_empty()\n        || fir.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {\n            matches!(&instruction.kind, FirInstructionKind::CallClosure { .. })\n        })\n    {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
if text.count(old) != 1:
    raise SystemExit(f"expected scalar routing anchor once, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))

# The closure-aware lowerer previously treated an empty local closure table as
# proof that no closure semantics were needed. Higher-order helper functions
# violate that assumption: they can invoke a closure parameter without defining
# a closure expression of their own.
path = ROOT / "crates/forge-codegen-cranelift/src/function_c14_closure.rs"
text = path.read_text()
old = '''    if fir.closures.is_empty() {\n        return lower_function_c11c(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
new = '''    let has_closure_call = fir\n        .blocks\n        .iter()\n        .flat_map(|block| &block.instructions)\n        .any(|instruction| matches!(&instruction.kind, FirInstructionKind::CallClosure { .. }));\n    if fir.closures.is_empty() && !has_closure_call {\n        return lower_function_c11c(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
if text.count(old) != 1:
    raise SystemExit(f"expected closure routing anchor once, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))

# Keep this routing patch separate from the ABI materializer so focused failures
# identify dispatch mistakes independently from environment layout mistakes.
print("materialized higher-order closure-call routing")
