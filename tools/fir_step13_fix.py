from pathlib import Path
p = Path('tools/fir_step13.py')
s = p.read_text()
old = '''replace_once(\n    typecheck,\n    ''' + "'''        } else if let Some(plan) = resolved_match {\\n            TypedExprKind::ResolvedMatch {\\n                plan,\\n                hir: expr.clone(),\\n            }\\n        } else if let Some((operation, provenance)) = unsafe_operation {'''" + ''',\n    ''' + "'''        } else if let Some(plan) = resolved_match {\\n            TypedExprKind::ResolvedMatch {\\n                plan,\\n                hir: expr.clone(),\\n            }\\n        } else if let Some(access) = resolved_bitfield {\\n            TypedExprKind::ResolvedBitField {\\n                access,\\n                hir: expr.clone(),\\n            }\\n        } else if let Some((operation, provenance)) = unsafe_operation {'''" + ''')'''
new = '''replace_once(\n    typecheck,\n    ''' + "'''        let base_kind = if let Some((operation, provenance)) = resolved_unsafe {\\n            TypedExprKind::UnsafeOperation {'''" + ''',\n    ''' + "'''        let base_kind = if let Some(access) = resolved_bitfield {\\n            TypedExprKind::ResolvedBitField {\\n                access,\\n                hir: expr.clone(),\\n            }\\n        } else if let Some((operation, provenance)) = resolved_unsafe {\\n            TypedExprKind::UnsafeOperation {'''" + ''')'''
if old not in s:
    raise SystemExit('old migration block not found')
p.write_text(s.replace(old, new, 1))
