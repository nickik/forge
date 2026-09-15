use std::fmt;

/// Backend failures are code-generation failures only. They must never be used
/// to defer or re-run Forge semantic analysis below FIR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    InvalidFir {
        diagnostic_count: usize,
    },
    UnsupportedFir {
        component: &'static str,
    },
    UnsupportedType {
        kind: &'static str,
    },
    SemanticTypeLeak {
        kind: &'static str,
    },
    UnsupportedTargetLayout {
        pointer_bits: u16,
    },
    InvalidTarget {
        triple: &'static str,
        message: String,
    },
    InvalidFirShape {
        message: String,
    },
    InvalidConstant {
        text: String,
    },
    UnsupportedInstruction {
        kind: &'static str,
    },
    UnsupportedControlFlow {
        feature: &'static str,
    },
    Cranelift {
        message: String,
    },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFir { diagnostic_count } => write!(
                f,
                "FIR verification failed with {diagnostic_count} diagnostic(s)"
            ),
            Self::UnsupportedFir { component } => {
                write!(f, "FIR component is not lowered to CLIF yet: {component}")
            }
            Self::UnsupportedType { kind } => {
                write!(f, "FIR type is not lowered to a CLIF value yet: {kind}")
            }
            Self::SemanticTypeLeak { kind } => write!(
                f,
                "frontend-only semantic type leaked into FIR codegen: {kind}"
            ),
            Self::UnsupportedTargetLayout { pointer_bits } => write!(
                f,
                "unsupported Forge target pointer width: {pointer_bits} bits"
            ),
            Self::InvalidTarget { triple, message } => {
                write!(f, "invalid Cranelift target {triple}: {message}")
            }
            Self::InvalidFirShape { message } => write!(f, "invalid FIR shape: {message}"),
            Self::InvalidConstant { text } => write!(f, "invalid FIR constant `{text}`"),
            Self::UnsupportedInstruction { kind } => {
                write!(f, "FIR instruction is not lowered to CLIF yet: {kind}")
            }
            Self::UnsupportedControlFlow { feature } => {
                write!(f, "FIR control-flow shape is not supported by C3: {feature}")
            }
            Self::Cranelift { message } => write!(f, "Cranelift error: {message}"),
        }
    }
}

impl std::error::Error for BackendError {}
