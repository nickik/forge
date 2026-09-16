from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Ordinary scalar lowering must hand every function that carries closure values
# to the closure-aware path. This includes both invokers (CallClosure) and pure
# pass-through functions that only load/store/forward closure-typed locals.
path = ROOT / "crates/forge-codegen-cranelift/src/function_c14_scalar.rs"
text = path.read_text()
old = '''    if !fir.closures.is_empty() {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
new = '''    let has_closure_value = fir\n        .locals\n        .values()\n        .any(|local| matches!(&local.ty, Ty::Closure { .. }));\n    let has_closure_call = fir\n        .blocks\n        .iter()\n        .flat_map(|block| &block.instructions)\n        .any(|instruction| matches!(&instruction.kind, FirInstructionKind::CallClosure { .. }));\n    if !fir.closures.is_empty() || has_closure_value || has_closure_call {\n        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
if text.count(old) != 1:
    raise SystemExit(f"expected scalar routing anchor once, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))

# The closure-aware lowerer must likewise retain control when a function only
# carries a closure value across an ordinary function-call boundary.
path = ROOT / "crates/forge-codegen-cranelift/src/function_c14_closure.rs"
text = path.read_text()
old = '''    if fir.closures.is_empty() {\n        return lower_function_c11c(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
new = '''    let has_closure_value = fir\n        .locals\n        .values()\n        .any(|local| matches!(&local.ty, Ty::Closure { .. }));\n    let has_closure_call = fir\n        .blocks\n        .iter()\n        .flat_map(|block| &block.instructions)\n        .any(|instruction| matches!(&instruction.kind, FirInstructionKind::CallClosure { .. }));\n    if fir.closures.is_empty() && !has_closure_value && !has_closure_call {\n        return lower_function_c11c(fir, all_functions, all_globals, definitions, types, isa);\n    }'''
if text.count(old) != 1:
    raise SystemExit(f"expected closure routing anchor once, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))

print("materialized closure-value ABI routing")
