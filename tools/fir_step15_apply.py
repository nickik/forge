from pathlib import Path
import subprocess

p = Path("tools/fir_step15.py")
text = p.read_text()
text = text.replace(
    '    "    TypedExprKind, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope,\\n    UnsafeOperationKind, UnsafeProvenance,",\n    "    TypedExprKind, TypedGlobalInitializer, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan,\\n    TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,",',
    '    "    TypedContextScope, TypedExpr, TypedExprKind, TypedMatchArmPlan, TypedMatchBinding,\\n    TypedMatchPlan, TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,",\n    "    TypedContextScope, TypedExpr, TypedExprKind, TypedGlobalInitializer, TypedMatchArmPlan,\\n    TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,",',
)
p.write_text(text)
subprocess.run(["python3", "tools/fir_step15.py"], check=True)
