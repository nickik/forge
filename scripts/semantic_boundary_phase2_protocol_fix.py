from pathlib import Path

p = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = p.read_text()
text = text.replace('self.env.lookup_method(&channel_ty, "recv")', 'self.env.lookup_method(&channel_ty, "receive")')
text = text.replace('select channel `recv` method must take only its receiver', 'select channel `receive` method must take only its receiver')
text = text.replace('does not provide the required `recv` method', 'does not provide the required `receive` method')
p.write_text(text)

p = Path("crates/forge-frontend/tests/typecheck.rs")
text = p.read_text().replace('fn recv(self: &Jobs) -> u32', 'fn receive(self: &Jobs) -> u32')
p.write_text(text)
print("select protocol keyword collision fixed")
