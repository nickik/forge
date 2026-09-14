from pathlib import Path
import subprocess

p = Path("tools/fir_step16.py")
text = p.read_text()
text = text.replace(
    '''\n# FdnValue is no longer needed by FIR itself after duration normalization.\nreplace(\n    "crates/forge-frontend/src/fir_v1.rs",\n    "    ast::{BinaryOp, FdnValue, MetadataArg, Span, UnaryOp},",\n    "    ast::{BinaryOp, MetadataArg, Span, UnaryOp},",\n)\n''',
    "\n",
)
p.write_text(text)
subprocess.run(["python3", "tools/fir_step16.py"], check=True)
