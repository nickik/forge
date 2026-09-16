from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return (ROOT / path).read_text()


def write(path, text):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def replace_once(text, old, new, path):
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, found {text.count(old)}")
    return text.replace(old, new, 1)


# Keep the generated semantic comments aligned with the new ABI rather than the
# old function-local tag-dispatch implementation.
p = "crates/forge-codegen-cranelift/src/completion.rs"
s = read(p)
old = '''/// Lift capture-free anonymous functions marked as first-class `fn(...)`
/// values into deterministic synthetic module functions.
///
/// The frontend/FIR has already proved that `function_pointer` closures have
/// no environment captures. Native codegen therefore does not need a closure
/// ABI: the value is an ordinary function address and calls use the existing
/// C9 indirect-function ABI.
pub(crate) fn lift_captured_closure_values(
'''
new = '''/// Lift non-escaping captured closures into deterministic synthetic module
/// functions using the C14 environment-pointer ABI.
///
/// The caller retains lexical ownership of the stack environment. The lifted
/// function receives that environment pointer as its hidden first parameter;
/// returning a closure or storing one globally remains rejected until Forge has
/// an explicit owned-callable allocation/lifetime model.
pub(crate) fn lift_captured_closure_values(
'''
s = replace_once(s, old, new, p)
anchor = '''pub(crate) fn lift_capture_free_function_values(
'''
comment = '''/// Lift capture-free anonymous functions marked as first-class `fn(...)`
/// values into deterministic synthetic module functions.
///
/// The frontend/FIR has already proved that `function_pointer` closures have
/// no environment captures. Native codegen therefore does not need a closure
/// ABI: the value is an ordinary function address and calls use the existing
/// C9 indirect-function ABI.
'''
s = replace_once(s, anchor, comment + anchor, p)
write(p, s)

p = "crates/forge-codegen-cranelift/src/function_c14_closure.rs"
s = read(p)
old = '''/// C14 lowers Forge v1 captured closures as strictly function-local values.
///
/// A closure value is a pointer to a stack environment owned by the enclosing
/// native function. Calls dispatch to cloned closure FIR blocks inside that
/// same CLIF function. No closure environment is heap allocated and no
/// closure calling convention or externally visible closure symbol exists.
'''
new = '''/// C14 lowers Forge v1 captured closures as non-escaping environment pointers.
///
/// A closure value points at lexical stack storage whose first word is the
/// lifted closure code pointer and whose remaining fields are captures. A call
/// loads that code pointer and performs an indirect native call with the
/// environment pointer as the hidden first argument. No environment is heap
/// allocated, so the value may cross synchronous call boundaries but may not
/// outlive the creating activation.
'''
s = replace_once(s, old, new, p)
write(p, s)

# Explicitly prove that the stack-backed ABI does not accidentally permit a
# captured environment to escape its creating activation.
write(
    "examples/c14-native-spec/reject_escaping_closure.fg",
    '''module examples.c14_native_spec.reject_escaping_closure;

fn make_adder(base: i32) -> closure(i32) -> i32 {
    val add = [base](value: i32) -> i32 {
        return base + value;
    };
    return add;
}

fn main() -> i32 {
    return 0;
}
''',
)

p = "scripts/run-c14-native-spec.sh"
s = read(p)
anchor = '''echo "native-spec expected capability rejection: required tail call"
'''
block = '''echo "native-spec expected capability rejection: escaping captured closure"
escape_error="$(mktemp)"
if "$FORGEC" "${common[@]}" --run \\
  "$ROOT/examples/c14-native-spec/reject_escaping_closure.fg" \\
  >/dev/null 2>"$escape_error"; then
  echo "escaping captured closure unexpectedly compiled" >&2
  rm -f "$escape_error"
  exit 1
fi
if ! grep -q "escaping closure return requires heap/lifetime support" "$escape_error"; then
  echo "escaping captured closure failed for the wrong reason:" >&2
  cat "$escape_error" >&2
  rm -f "$escape_error"
  exit 1
fi
rm -f "$escape_error"

'''
s = replace_once(s, anchor, block + anchor, p)
write(p, s)

print("polished captured closure ABI and escape regression")
