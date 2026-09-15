use std::collections::BTreeMap;

use forge_frontend::{ConstValue, DefId};

/// A symbol address embedded in statically-emitted Forge data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StaticSymbol {
    Function(DefId),
    Global(DefId),
}

/// Code-generation-facing constant tree used for static global initialization.
///
/// The frontend's semantic `ConstValue` deliberately remains scalar. This FIR
/// side table adds the structural forms needed by native object emission
/// without teaching the backend about AST/HIR syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticValue {
    Scalar(ConstValue),
    Zero,
    Array(Vec<StaticValue>),
    Aggregate {
        variant: Option<String>,
        fields: BTreeMap<String, StaticValue>,
    },
    Address {
        target: StaticSymbol,
        addend: i64,
    },
}

/// Optional code-generation side-table entry for a FIR global whose static
/// value is richer than the scalar `FirGlobal::constant` representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticGlobalInitializer {
    pub value: StaticValue,
    pub writable: bool,
}

pub type StaticGlobalInitializerTable = BTreeMap<DefId, StaticGlobalInitializer>;
