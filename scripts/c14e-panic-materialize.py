from pathlib import Path

core = Path("lib/core.fg")
text = core.read_text()
anchor = """// core defines the information and may raise panic paths, but does not decide
// whether a process exits, a kernel halts, firmware resets, or a debugger runs.
"""
addition = anchor + """

nfn __forge_panic(info: &PanicInfo) -> never {
    return __forge_panic(:info=info);
}

pub fn panic(message: str) -> never {
    val info: PanicInfo = PanicInfo{
        kind: PanicKind::Explicit,
        message: message,
        location: None,
    };
    return __forge_panic(:info=&info);
}
"""
assert text.count(anchor) == 1
core.write_text(text.replace(anchor, addition, 1))

runtime = Path("crates/forge-compiler/src/hosted_runtime.rs")
text = runtime.read_text()
arm = """        let definition = match name.as_str() {
  \"__forge_console_write\" => format!(
"""
replacement = """        let definition = match name.as_str() {
  \"__forge_panic\" => format!(
      \"__attribute__((noreturn)) void {symbol}(const void *info) {{ (void)info; forge_host_abort(); }}\\n\"
  ),
  \"__forge_console_write\" => format!(
"""
assert text.count(arm) == 1
text = text.replace(arm, replacement, 1)
whitelist = """            \"__forge_console_write\"
                | \"__forge_args_count\"
"""
whitelist_replacement = """            \"__forge_panic\"
                | \"__forge_console_write\"
                | \"__forge_args_count\"
"""
assert text.count(whitelist) == 1
runtime.write_text(text.replace(whitelist, whitelist_replacement, 1))

script = Path("scripts/run-c14-native-spec.sh")
text = script.read_text()
traps = """for source in \\
  \"$ROOT/examples/c14-native-spec/trap_checked_add.fg\" \\
  \"$ROOT/examples/c14-native-spec/trap_div_zero.fg\"; do
"""
traps_replacement = """for source in \\
  \"$ROOT/examples/c14-native-spec/trap_checked_add.fg\" \\
  \"$ROOT/examples/c14-native-spec/trap_div_zero.fg\" \\
  \"$ROOT/examples/c14-native-spec/trap_panic.fg\"; do
"""
assert text.count(traps) == 1
script.write_text(text.replace(traps, traps_replacement, 1))

Path("examples/c14-native-spec/trap_panic.fg").write_text("""module examples.c14_native_spec.trap_panic;

import core;

fn main() -> i32 {
    core.panic(\"explicit native panic\");
    return 99;
}
""")
