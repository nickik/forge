from pathlib import Path

path = Path("crates/forge-frontend/tests/typecheck.rs")
text = path.read_text()
old = '''    assert!(maybe.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedCall {
            argument_parameters,
            ..
        } if argument_parameters == &vec![1, 0]
    )));
'''
new = '''    assert!(maybe.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedCall { arguments, .. }
            if matches!(
                arguments.as_slice(),
                [
                    forge_frontend::ResolvedCallArgument::Explicit { argument: 1 },
                    forge_frontend::ResolvedCallArgument::Explicit { argument: 0 },
                ]
            )
    )));
'''
if old not in text:
    raise SystemExit("old typed call boundary assertion not found")
path.write_text(text.replace(old, new, 1))
