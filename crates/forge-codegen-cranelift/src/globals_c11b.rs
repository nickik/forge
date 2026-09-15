use std::collections::{BTreeMap, BTreeSet};

use forge_fir::{
    verify_fir_module, ConstValue, DefId, FirModule, IntWidth, Layout, LayoutEngine, LayoutKind,
    LayoutTarget, StaticGlobalInitializer, StaticGlobalInitializerTable, StaticSymbol, StaticValue,
    SumEncoding, Ty, TypeDefinitionKind, TypeDefinitionTable,
};

use crate::object::ObjectLinkage;
use crate::{BackendError, CraneliftBackend, CraneliftTarget};

/// How a Forge global obtains its initial value before ordinary program code
/// observes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalInitialization {
    /// Scalar semantic constant inherited directly from ordinary FIR.
    Constant(ConstValue),
    /// Rich static initializer used for aggregate data and symbol addresses.
    Static(StaticGlobalInitializer),
    /// Storage starts zeroed; C11d executes the initializer later.
    Runtime {
        dependencies: Vec<DefId>,
        order: u32,
    },
    /// Uninitialized/zero-initialized storage.
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalStorageClass {
    ReadOnlyData,
    WritableData,
    ZeroFill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedStaticRelocation {
    offset: u64,
    target: StaticSymbol,
    addend: i64,
    width: u8,
}

impl PreparedStaticRelocation {
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    pub const fn target(&self) -> StaticSymbol {
        self.target
    }

    pub const fn addend(&self) -> i64 {
        self.addend
    }

    pub const fn width(&self) -> u8 {
        self.width
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedStaticData {
    bytes: Vec<u8>,
    relocations: Vec<PreparedStaticRelocation>,
}

impl PreparedStaticData {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn relocations(&self) -> &[PreparedStaticRelocation] {
        &self.relocations
    }
}

/// C9-layout-complete representation of one FIR global.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedGlobal {
    owner: DefId,
    ty: Ty,
    layout: Layout,
    initialization: GlobalInitialization,
    storage: GlobalStorageClass,
    static_data: Option<PreparedStaticData>,
}

impl PreparedGlobal {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn ty(&self) -> &Ty {
        &self.ty
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn initialization(&self) -> &GlobalInitialization {
        &self.initialization
    }

    pub const fn storage(&self) -> GlobalStorageClass {
        self.storage
    }

    /// Fully serialized C9 memory image for `.rodata`/`.data` globals.
    pub fn static_data(&self) -> Option<&PreparedStaticData> {
        self.static_data.as_ref()
    }
}

#[derive(Clone, Debug)]
pub struct PreparedGlobals {
    target: CraneliftTarget,
    globals: BTreeMap<DefId, PreparedGlobal>,
    init_order: Vec<DefId>,
}

impl PreparedGlobals {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn globals(&self) -> &BTreeMap<DefId, PreparedGlobal> {
        &self.globals
    }

    pub fn global(&self, owner: DefId) -> Option<&PreparedGlobal> {
        self.globals.get(&owner)
    }

    pub fn init_order(&self) -> &[DefId] {
        &self.init_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalObjectSymbol {
    owner: DefId,
    name: String,
    linkage: ObjectLinkage,
    layout: Layout,
    initialization: GlobalInitialization,
    storage: GlobalStorageClass,
    static_data: Option<PreparedStaticData>,
}

impl GlobalObjectSymbol {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn linkage(&self) -> ObjectLinkage {
        self.linkage
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn initialization(&self) -> &GlobalInitialization {
        &self.initialization
    }

    pub const fn storage(&self) -> GlobalStorageClass {
        self.storage
    }

    pub fn static_data(&self) -> Option<&PreparedStaticData> {
        self.static_data.as_ref()
    }
}

#[derive(Clone, Debug)]
pub struct GlobalObjectPlan {
    target: CraneliftTarget,
    symbols: BTreeMap<DefId, GlobalObjectSymbol>,
}

impl GlobalObjectPlan {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn symbols(&self) -> &BTreeMap<DefId, GlobalObjectSymbol> {
        &self.symbols
    }

    pub fn symbol(&self, owner: DefId) -> Option<&GlobalObjectSymbol> {
        self.symbols.get(&owner)
    }
}

impl CraneliftBackend {
    pub fn prepare_globals(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<PreparedGlobals, BackendError> {
        self.prepare_globals_with_static_initializers(
            module,
            definitions,
            &StaticGlobalInitializerTable::new(),
        )
    }

    /// Prepare C9 layouts plus C11b static bytes/relocations. Rich static
    /// initializers are a code-generation side table, analogous to the C9
    /// nominal-type table: language semantics have already been resolved.
    pub fn prepare_globals_with_static_initializers(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
        static_initializers: &StaticGlobalInitializerTable,
    ) -> Result<PreparedGlobals, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        for owner in module.globals.keys() {
            if module.functions.contains_key(owner) {
                return Err(shape(format!(
                    "definition {owner:?} appears as both a function and a global"
                )));
            }
        }
        for owner in static_initializers.keys() {
            if !module.globals.contains_key(owner) {
                return Err(shape(format!(
                    "static initializer refers to missing global {owner:?}"
                )));
            }
        }

        let positions: BTreeMap<DefId, u32> = module
            .global_init_order
            .iter()
            .copied()
            .enumerate()
            .map(|(index, owner)| {
                u32::try_from(index)
                    .map(|index| (owner, index))
                    .map_err(|_| shape("global initializer order exceeds u32"))
            })
            .collect::<Result<_, _>>()?;

        let layout_target = LayoutTarget::new(self.target().layout().pointer_bits);
        let mut layout_engine = LayoutEngine::new(layout_target, definitions);
        let mut globals = BTreeMap::new();

        for (owner, global) in &module.globals {
            let runtime_initializer = module.global_initializers.get(owner);
            let static_initializer = static_initializers.get(owner);
            if global.constant.is_some() && runtime_initializer.is_some() {
                return Err(shape(format!(
                    "global {owner:?} has both a compile-time value and a runtime initializer"
                )));
            }
            if static_initializer.is_some() && runtime_initializer.is_some() {
                return Err(shape(format!(
                    "global {owner:?} has both a static-data initializer and a runtime initializer"
                )));
            }
            if static_initializer.is_some() && global.constant.is_some() {
                return Err(shape(format!(
                    "global {owner:?} has both a scalar constant and a static-data initializer"
                )));
            }

            let layout = layout_engine.layout_of(&global.ty).map_err(|error| {
                shape(format!(
                    "global {owner:?} has invalid C9 layout for {:?}: {error}",
                    global.ty
                ))
            })?;

            let (initialization, storage, static_data) = if let Some(initializer) =
                static_initializer
            {
                let data = serialize_static_initializer(
                    *owner,
                    layout_target,
                    definitions,
                    &global.ty,
                    &initializer.value,
                )?;
                (
                    GlobalInitialization::Static(initializer.clone()),
                    if initializer.writable {
                        GlobalStorageClass::WritableData
                    } else {
                        GlobalStorageClass::ReadOnlyData
                    },
                    Some(data),
                )
            } else {
                match (&global.constant, runtime_initializer) {
                    (Some(constant), None) => {
                        let data = serialize_static_initializer(
                            *owner,
                            layout_target,
                            definitions,
                            &global.ty,
                            &StaticValue::Scalar(constant.clone()),
                        )?;
                        (
                            GlobalInitialization::Constant(constant.clone()),
                            GlobalStorageClass::ReadOnlyData,
                            Some(data),
                        )
                    }
                    (None, Some(initializer)) => {
                        let order = positions.get(owner).copied().ok_or_else(|| {
                            shape(format!(
                                "runtime initializer for global {owner:?} is missing from global_init_order"
                            ))
                        })?;
                        (
                            GlobalInitialization::Runtime {
                                dependencies: initializer.dependencies.clone(),
                                order,
                            },
                            GlobalStorageClass::ZeroFill,
                            None,
                        )
                    }
                    (None, None) => (
                        GlobalInitialization::Zero,
                        GlobalStorageClass::ZeroFill,
                        None,
                    ),
                    (Some(_), Some(_)) => unreachable!("constant/runtime conflict rejected above"),
                }
            };

            globals.insert(
                *owner,
                PreparedGlobal {
                    owner: *owner,
                    ty: global.ty.clone(),
                    layout,
                    initialization,
                    storage,
                    static_data,
                },
            );
        }

        for owner in &module.global_init_order {
            if !globals.contains_key(owner) {
                return Err(shape(format!(
                    "global initializer order contains missing global {owner:?}"
                )));
            }
        }

        Ok(PreparedGlobals {
            target: self.target(),
            globals,
            init_order: module.global_init_order.clone(),
        })
    }

    pub fn plan_global_objects(
        &self,
        prepared: &PreparedGlobals,
    ) -> Result<GlobalObjectPlan, BackendError> {
        self.plan_global_objects_with_exports(prepared, std::iter::empty())
    }

    pub fn plan_global_objects_with_exports<I>(
        &self,
        prepared: &PreparedGlobals,
        exports: I,
    ) -> Result<GlobalObjectPlan, BackendError>
    where
        I: IntoIterator<Item = DefId>,
    {
        if prepared.target() != self.target() {
            return Err(shape(format!(
                "prepared globals target {:?} does not match object-plan target {:?}",
                prepared.target(),
                self.target()
            )));
        }

        let exports: BTreeSet<DefId> = exports.into_iter().collect();
        for owner in &exports {
            if !prepared.globals().contains_key(owner) {
                return Err(shape(format!("cannot export missing FIR global {owner:?}")));
            }
        }

        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::new();
        for (owner, global) in prepared.globals() {
            let name = forge_global_symbol(*owner);
            if !names.insert(name.clone()) {
                return Err(shape(format!(
                    "duplicate Forge global object symbol generated for {owner:?}: {name}"
                )));
            }
            symbols.insert(
                *owner,
                GlobalObjectSymbol {
                    owner: *owner,
                    name,
                    linkage: if exports.contains(owner) {
                        ObjectLinkage::Export
                    } else {
                        ObjectLinkage::Local
                    },
                    layout: global.layout().clone(),
                    initialization: global.initialization().clone(),
                    storage: global.storage(),
                    static_data: global.static_data().cloned(),
                },
            );
        }

        Ok(GlobalObjectPlan {
            target: self.target(),
            symbols,
        })
    }
}

fn serialize_static_initializer(
    owner: DefId,
    target: LayoutTarget,
    definitions: &TypeDefinitionTable,
    ty: &Ty,
    value: &StaticValue,
) -> Result<PreparedStaticData, BackendError> {
    let mut layouts = LayoutEngine::new(target, definitions);
    let layout = layouts.layout_of(ty).map_err(|error| {
        shape(format!(
            "global {owner:?} static initializer has invalid C9 layout for {ty:?}: {error}"
        ))
    })?;
    let size = usize::try_from(layout.size)
        .map_err(|_| shape(format!("global {owner:?} static data exceeds usize")))?;
    let mut output = PreparedStaticData {
        bytes: vec![0; size],
        relocations: Vec::new(),
    };
    serialize_value(
        owner,
        definitions,
        &mut layouts,
        ty,
        &layout,
        value,
        0,
        &mut output,
    )?;
    output
        .relocations
        .sort_by_key(|relocation| (relocation.offset, relocation.target, relocation.addend));
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn serialize_value(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    ty: &Ty,
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    if matches!(value, StaticValue::Zero) {
        return Ok(());
    }

    match ty {
        Ty::Bool => match value {
            StaticValue::Scalar(ConstValue::Bool { value }) => {
                write_integer(owner, output, base, 1, if *value { 1 } else { 0 })
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Char => match value {
            StaticValue::Scalar(ConstValue::Char { value }) => {
                write_integer(owner, output, base, 4, u128::from(u32::from(*value)))
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Byte => match value {
            StaticValue::Scalar(ConstValue::Integer { value }) => {
                serialize_integer(owner, output, base, 1, false, *value)
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Int { signed, width } => match value {
            StaticValue::Scalar(ConstValue::Integer { value }) => {
                serialize_integer(owner, output, base, layout.size, *signed, *value)
            }
            StaticValue::Address { target, addend } if *width == IntWidth::Pointer => {
                serialize_address(owner, output, base, layout.size, *target, *addend)
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Duration => match value {
            StaticValue::Scalar(ConstValue::Integer { value }) => {
                serialize_integer(owner, output, base, layout.size, true, *value)
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => match value {
            StaticValue::Address { target, addend } => {
                serialize_address(owner, output, base, layout.size, *target, *addend)
            }
            _ => type_mismatch(owner, ty, value),
        },
        Ty::Array { element, length } => {
            let StaticValue::Array(items) = value else {
                return type_mismatch(owner, ty, value);
            };
            let Some(length) = length else {
                return Err(shape(format!(
                    "global {owner:?} static array has unknown length"
                )));
            };
            if items.len() as u64 != *length {
                return Err(shape(format!(
                    "global {owner:?} static array has {} items but type requires {length}",
                    items.len()
                )));
            }
            let LayoutKind::Array { stride, .. } = &layout.kind else {
                return Err(shape(format!(
                    "global {owner:?} C9 array layout kind mismatch"
                )));
            };
            let element_layout = layouts.layout_of(element).map_err(|error| {
                shape(format!(
                    "global {owner:?} array element layout failed: {error}"
                ))
            })?;
            for (index, item) in items.iter().enumerate() {
                serialize_value(
                    owner,
                    definitions,
                    layouts,
                    element,
                    &element_layout,
                    item,
                    base + *stride * index as u64,
                    output,
                )?;
            }
            Ok(())
        }
        Ty::Slice { element, .. } => serialize_slice(
            owner,
            definitions,
            layouts,
            element,
            layout,
            value,
            base,
            output,
        ),
        Ty::Optional { inner } => serialize_optional(
            owner,
            definitions,
            layouts,
            inner,
            layout,
            value,
            base,
            output,
        ),
        Ty::Result { ok, error } => serialize_result(
            owner,
            definitions,
            layouts,
            ok,
            error,
            layout,
            value,
            base,
            output,
        ),
        Ty::Nominal(type_owner) => {
            let kind = definitions
                .get(type_owner)
                .ok_or_else(|| {
                    shape(format!(
                        "global {owner:?} static initializer is missing nominal definition {type_owner:?}"
                    ))
                })?
                .kind
                .clone();
            match kind {
                TypeDefinitionKind::Alias { target } => {
                    let child = layouts.layout_of(&target).map_err(|error| {
                        shape(format!("global {owner:?} alias layout failed: {error}"))
                    })?;
                    serialize_value(
                        owner,
                        definitions,
                        layouts,
                        &target,
                        &child,
                        value,
                        base,
                        output,
                    )
                }
                TypeDefinitionKind::Distinct { underlying } => {
                    let child = layouts.layout_of(&underlying).map_err(|error| {
                        shape(format!("global {owner:?} distinct layout failed: {error}"))
                    })?;
                    serialize_value(
                        owner,
                        definitions,
                        layouts,
                        &underlying,
                        &child,
                        value,
                        base,
                        output,
                    )
                }
                TypeDefinitionKind::BitStruct { storage } => {
                    let child = layouts.layout_of(&storage).map_err(|error| {
                        shape(format!("global {owner:?} bitstruct layout failed: {error}"))
                    })?;
                    serialize_value(
                        owner,
                        definitions,
                        layouts,
                        &storage,
                        &child,
                        value,
                        base,
                        output,
                    )
                }
                TypeDefinitionKind::Struct { fields } => serialize_struct(
                    owner,
                    definitions,
                    layouts,
                    &fields,
                    layout,
                    value,
                    base,
                    output,
                ),
                TypeDefinitionKind::Enum { variants } => {
                    let StaticValue::Aggregate { variant, fields } = value else {
                        return type_mismatch(owner, ty, value);
                    };
                    if !fields.is_empty() {
                        return Err(shape(format!(
                            "global {owner:?} enum static value unexpectedly has fields"
                        )));
                    }
                    let name = variant.as_ref().ok_or_else(|| {
                        shape(format!(
                            "global {owner:?} enum static value is missing variant"
                        ))
                    })?;
                    let index = variants
                        .iter()
                        .position(|candidate| candidate.name == *name)
                        .ok_or_else(|| {
                            shape(format!(
                                "global {owner:?} enum static value names unknown variant {name}"
                            ))
                        })? as u32;
                    let LayoutKind::Enum { tag } = &layout.kind else {
                        return Err(shape(format!(
                            "global {owner:?} C9 enum layout kind mismatch"
                        )));
                    };
                    if let Some(tag) = tag {
                        write_integer(
                            owner,
                            output,
                            base + tag.offset,
                            u64::from(tag.size),
                            u128::from(index),
                        )?;
                    }
                    Ok(())
                }
                TypeDefinitionKind::Tagged { variants } => serialize_tagged(
                    owner,
                    definitions,
                    layouts,
                    &variants,
                    layout,
                    value,
                    base,
                    output,
                ),
            }
        }
        Ty::Void | Ty::Never if layout.size == 0 => Ok(()),
        Ty::Void
        | Ty::Never
        | Ty::Float { .. }
        | Ty::Str
        | Ty::ContextSlot { .. }
        | Ty::Closure { .. }
        | Ty::Error
        | Ty::Unknown
        | Ty::IntLiteral
        | Ty::FloatLiteral
        | Ty::NoneLiteral => type_mismatch(owner, ty, value),
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_struct(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    fields: &[forge_fir::TypeFieldDefinition],
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    let StaticValue::Aggregate {
        variant: None,
        fields: values,
    } = value
    else {
        return Err(shape(format!(
            "global {owner:?} struct static initializer is not a plain aggregate"
        )));
    };
    if values.len() != fields.len() {
        return Err(shape(format!(
            "global {owner:?} struct static initializer has {} fields but type requires {}",
            values.len(),
            fields.len()
        )));
    }
    for field in fields {
        let value = values.get(&field.name).ok_or_else(|| {
            shape(format!(
                "global {owner:?} struct static initializer is missing field {}",
                field.name
            ))
        })?;
        let placed = layout.field(&field.name).ok_or_else(|| {
            shape(format!(
                "global {owner:?} C9 layout is missing field {}",
                field.name
            ))
        })?;
        let child = layouts.layout_of(&field.ty).map_err(|error| {
            shape(format!(
                "global {owner:?} field {} layout failed: {error}",
                field.name
            ))
        })?;
        serialize_value(
            owner,
            definitions,
            layouts,
            &field.ty,
            &child,
            value,
            base + placed.offset,
            output,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn serialize_tagged(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    variants: &[forge_fir::TypeVariantDefinition],
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    let StaticValue::Aggregate {
        variant: Some(name),
        fields: values,
    } = value
    else {
        return Err(shape(format!(
            "global {owner:?} tagged static initializer is missing a variant"
        )));
    };
    let index = variants
        .iter()
        .position(|candidate| candidate.name == *name)
        .ok_or_else(|| {
            shape(format!(
                "global {owner:?} tagged static initializer names unknown variant {name}"
            ))
        })? as u32;
    let definition = &variants[index as usize];
    if values.len() != definition.fields.len() {
        return Err(shape(format!(
            "global {owner:?} tagged variant {name} has {} fields but type requires {}",
            values.len(),
            definition.fields.len()
        )));
    }
    let LayoutKind::Tagged {
        encoding,
        variants: placed_variants,
    } = &layout.kind
    else {
        return Err(shape(format!(
            "global {owner:?} C9 tagged layout kind mismatch"
        )));
    };
    encode_sum_discriminant(owner, output, base, encoding, index)?;
    let placed = placed_variants
        .iter()
        .find(|candidate| candidate.name == *name)
        .ok_or_else(|| {
            shape(format!(
                "global {owner:?} C9 tagged layout is missing variant {name}"
            ))
        })?;
    for field in &definition.fields {
        let value = values.get(&field.name).ok_or_else(|| {
            shape(format!(
                "global {owner:?} tagged variant {name} is missing field {}",
                field.name
            ))
        })?;
        let field_layout = placed
            .fields
            .iter()
            .find(|candidate| candidate.name == field.name)
            .ok_or_else(|| {
                shape(format!(
                    "global {owner:?} C9 tagged layout is missing field {}",
                    field.name
                ))
            })?;
        let child = layouts.layout_of(&field.ty).map_err(|error| {
            shape(format!(
                "global {owner:?} tagged field {} layout failed: {error}",
                field.name
            ))
        })?;
        serialize_value(
            owner,
            definitions,
            layouts,
            &field.ty,
            &child,
            value,
            base + field_layout.offset,
            output,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn serialize_optional(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    inner: &Ty,
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    let StaticValue::Aggregate {
        variant: Some(name),
        fields,
    } = value
    else {
        let expected = Ty::Optional {
            inner: Box::new(inner.clone()),
        };
        return type_mismatch(owner, &expected, value);
    };
    let LayoutKind::Optional { encoding } = &layout.kind else {
        return Err(shape(format!(
            "global {owner:?} C9 optional layout kind mismatch"
        )));
    };
    match name.as_str() {
        "None" => {
            if !fields.is_empty() {
                return Err(shape(format!(
                    "global {owner:?} None static initializer unexpectedly has fields"
                )));
            }
            encode_sum_discriminant(owner, output, base, encoding, 0)
        }
        "Some" => {
            if fields.len() != 1 {
                return Err(shape(format!(
                    "global {owner:?} Some static initializer must contain exactly value"
                )));
            }
            encode_sum_discriminant(owner, output, base, encoding, 1)?;
            let child = layouts.layout_of(inner).map_err(|error| {
                shape(format!(
                    "global {owner:?} optional payload layout failed: {error}"
                ))
            })?;
            let payload = fields.get("value").ok_or_else(|| {
                shape(format!(
                    "global {owner:?} Some static initializer is missing value"
                ))
            })?;
            serialize_value(
                owner,
                definitions,
                layouts,
                inner,
                &child,
                payload,
                base + sum_payload_offset(encoding, 1),
                output,
            )
        }
        _ => Err(shape(format!(
            "global {owner:?} optional static initializer names unknown variant {name}"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_result(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    ok: &Ty,
    error: &Ty,
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    let StaticValue::Aggregate {
        variant: Some(name),
        fields,
    } = value
    else {
        let expected = Ty::Result {
            ok: Box::new(ok.clone()),
            error: Box::new(error.clone()),
        };
        return type_mismatch(owner, &expected, value);
    };
    let LayoutKind::Result { encoding } = &layout.kind else {
        return Err(shape(format!(
            "global {owner:?} C9 result layout kind mismatch"
        )));
    };
    let (index, field_name, child_ty) = match name.as_str() {
        "Ok" => (0, "value", ok),
        "Err" => (1, "error", error),
        _ => {
            return Err(shape(format!(
                "global {owner:?} result static initializer names unknown variant {name}"
            )))
        }
    };
    if fields.len() != 1 {
        return Err(shape(format!(
            "global {owner:?} result variant {name} must contain exactly {field_name}"
        )));
    }
    encode_sum_discriminant(owner, output, base, encoding, index)?;
    let child = layouts.layout_of(child_ty).map_err(|error| {
        shape(format!(
            "global {owner:?} result payload layout failed: {error}"
        ))
    })?;
    let payload = fields.get(field_name).ok_or_else(|| {
        shape(format!(
            "global {owner:?} result variant {name} is missing {field_name}"
        ))
    })?;
    serialize_value(
        owner,
        definitions,
        layouts,
        child_ty,
        &child,
        payload,
        base + sum_payload_offset(encoding, index),
        output,
    )
}

#[allow(clippy::too_many_arguments)]
fn serialize_slice(
    owner: DefId,
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    element: &Ty,
    layout: &Layout,
    value: &StaticValue,
    base: u64,
    output: &mut PreparedStaticData,
) -> Result<(), BackendError> {
    let StaticValue::Aggregate {
        variant: None,
        fields,
    } = value
    else {
        return Err(shape(format!(
            "global {owner:?} slice static initializer is not a plain aggregate"
        )));
    };
    if fields.len() != 2 {
        return Err(shape(format!(
            "global {owner:?} slice static initializer requires data and len"
        )));
    }
    let LayoutKind::Slice {
        data_offset,
        len_offset,
    } = &layout.kind
    else {
        return Err(shape(format!(
            "global {owner:?} C9 slice layout kind mismatch"
        )));
    };
    let pointer_ty = Ty::Pointer {
        volatile: false,
        inner: Box::new(element.clone()),
    };
    let pointer_layout = layouts.layout_of(&pointer_ty).map_err(|error| {
        shape(format!(
            "global {owner:?} slice pointer layout failed: {error}"
        ))
    })?;
    let len_ty = Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    };
    let len_layout = layouts.layout_of(&len_ty).map_err(|error| {
        shape(format!(
            "global {owner:?} slice length layout failed: {error}"
        ))
    })?;
    serialize_value(
        owner,
        definitions,
        layouts,
        &pointer_ty,
        &pointer_layout,
        fields.get("data").ok_or_else(|| {
            shape(format!(
                "global {owner:?} slice static initializer is missing data"
            ))
        })?,
        base + *data_offset,
        output,
    )?;
    serialize_value(
        owner,
        definitions,
        layouts,
        &len_ty,
        &len_layout,
        fields.get("len").ok_or_else(|| {
            shape(format!(
                "global {owner:?} slice static initializer is missing len"
            ))
        })?,
        base + *len_offset,
        output,
    )
}

fn encode_sum_discriminant(
    owner: DefId,
    output: &mut PreparedStaticData,
    base: u64,
    encoding: &SumEncoding,
    variant: u32,
) -> Result<(), BackendError> {
    match encoding {
        SumEncoding::Single => {
            if variant == 0 {
                Ok(())
            } else {
                Err(shape(format!(
                    "global {owner:?} single-variant layout cannot encode variant {variant}"
                )))
            }
        }
        SumEncoding::Tagged { tag, .. } => write_integer(
            owner,
            output,
            base + tag.offset,
            u64::from(tag.size),
            u128::from(variant),
        ),
        SumEncoding::Niche {
            payload_variant,
            niche_offset,
            niche_bits,
            fieldless_values,
        } => {
            if variant == *payload_variant {
                return Ok(());
            }
            let niche_value = fieldless_values
                .iter()
                .find_map(|(candidate, value)| (*candidate == variant).then_some(*value))
                .ok_or_else(|| {
                    shape(format!(
                        "global {owner:?} niche layout cannot encode variant {variant}"
                    ))
                })?;
            if niche_bits % 8 != 0 {
                return Err(shape(format!(
                    "global {owner:?} niche width {niche_bits} is not byte-addressable"
                )));
            }
            write_integer(
                owner,
                output,
                base + *niche_offset,
                u64::from(*niche_bits / 8),
                niche_value,
            )
        }
    }
}

fn sum_payload_offset(encoding: &SumEncoding, variant: u32) -> u64 {
    match encoding {
        SumEncoding::Tagged { payload_offset, .. } => *payload_offset,
        SumEncoding::Niche {
            payload_variant, ..
        } if variant == *payload_variant => 0,
        SumEncoding::Niche { .. } | SumEncoding::Single => 0,
    }
}

fn serialize_address(
    owner: DefId,
    output: &mut PreparedStaticData,
    offset: u64,
    width: u64,
    target: StaticSymbol,
    addend: i64,
) -> Result<(), BackendError> {
    if !matches!(width, 4 | 8) {
        return Err(shape(format!(
            "global {owner:?} static address width {width} is unsupported"
        )));
    }
    checked_range(owner, output, offset, width)?;
    output.relocations.push(PreparedStaticRelocation {
        offset,
        target,
        addend,
        width: width as u8,
    });
    Ok(())
}

fn serialize_integer(
    owner: DefId,
    output: &mut PreparedStaticData,
    offset: u64,
    width: u64,
    signed: bool,
    value: i128,
) -> Result<(), BackendError> {
    let bits = width
        .checked_mul(8)
        .ok_or_else(|| shape(format!("global {owner:?} integer width overflow")))?;
    if bits == 0 || bits > 128 {
        return Err(shape(format!(
            "global {owner:?} integer width {width} is unsupported"
        )));
    }
    if signed {
        let min = if bits == 128 {
            i128::MIN
        } else {
            -(1i128 << (bits - 1))
        };
        let max = if bits == 128 {
            i128::MAX
        } else {
            (1i128 << (bits - 1)) - 1
        };
        if value < min || value > max {
            return Err(shape(format!(
                "global {owner:?} integer constant {value} does not fit signed {bits}-bit storage"
            )));
        }
    } else if value < 0 || (bits < 128 && value as u128 >= (1u128 << bits)) {
        return Err(shape(format!(
            "global {owner:?} integer constant {value} does not fit unsigned {bits}-bit storage"
        )));
    }
    write_integer(owner, output, offset, width, value as u128)
}

fn write_integer(
    owner: DefId,
    output: &mut PreparedStaticData,
    offset: u64,
    width: u64,
    value: u128,
) -> Result<(), BackendError> {
    let range = checked_range(owner, output, offset, width)?;
    let bytes = value.to_le_bytes();
    output.bytes[range].copy_from_slice(&bytes[..width as usize]);
    Ok(())
}

fn checked_range(
    owner: DefId,
    output: &PreparedStaticData,
    offset: u64,
    width: u64,
) -> Result<std::ops::Range<usize>, BackendError> {
    let end = offset
        .checked_add(width)
        .ok_or_else(|| shape(format!("global {owner:?} static data offset overflow")))?;
    let start = usize::try_from(offset)
        .map_err(|_| shape(format!("global {owner:?} static data offset exceeds usize")))?;
    let end = usize::try_from(end)
        .map_err(|_| shape(format!("global {owner:?} static data end exceeds usize")))?;
    if end > output.bytes.len() {
        return Err(shape(format!(
            "global {owner:?} static data write {start}..{end} exceeds {} bytes",
            output.bytes.len()
        )));
    }
    Ok(start..end)
}

fn type_mismatch<T>(owner: DefId, ty: &Ty, value: &StaticValue) -> Result<T, BackendError> {
    Err(shape(format!(
        "global {owner:?} static value {value:?} does not match FIR type {ty:?}"
    )))
}

fn forge_global_symbol(owner: DefId) -> String {
    format!("__forge_global_{:08x}", owner.0)
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
