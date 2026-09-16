use std::fs;
use std::path::PathBuf;

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = u32::from(chunk[0]);
        let b = u32::from(*chunk.get(1).unwrap_or(&0));
        let c = u32::from(*chunk.get(2).unwrap_or(&0));
        let bits = (a << 16) | (b << 8) | c;
        out.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
        if chunk.len() >= 2 {
            out.push(TABLE[((bits >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() == 3 {
            out.push(TABLE[(bits & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[test]
fn emit_c14e_rest_patched_fir_blob() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fir_v1.rs");
    let text = fs::read_to_string(path).expect("read FIR source");
    let old = r#"                if let Some(rest) = rest {
                    self.diagnostic(
                        pattern.span,
                        "fir/rest-pattern",
                        format!("irrefutable rest binding {rest:?} needs slice-view lowering"),
                    );
                }
"#;
    let new = r#"                if let Some(rest) = rest {
                    let rest_ty = self
                        .typed
                        .local_types
                        .get(rest)
                        .cloned()
                        .unwrap_or(Ty::Unknown);
                    let rest_value = self.emit_value(
                        pattern.span,
                        rest_ty,
                        FirInstructionKind::Subsequence {
                            base: value,
                            start: items.len() as u64,
                        },
                    );
                    self.store_local(pattern.span, *rest, rest_value);
                }
"#;
    assert_eq!(
        text.matches(old).count(),
        1,
        "unexpected rest lowering shape"
    );
    let patched = text.replacen(old, new, 1);
    assert!(!patched.contains("fir/rest-pattern"));
    println!("BEGIN_C14E_REST_FIR_BASE64");
    println!("{}", encode_base64(patched.as_bytes()));
    println!("END_C14E_REST_FIR_BASE64");
}