use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use forge_frontend::{
    DefId, IntWidth, Ty, TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition,
    TypeVariantDefinition,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutTarget {
    pub pointer_bits: u16,
}

impl LayoutTarget {
    pub const fn new(pointer_bits: u16) -> Self {
        Self { pointer_bits }
    }

    fn pointer_bytes(self) -> Result<u64, LayoutError> {
        match self.pointer_bits {
            32 => Ok(4),
            64 => Ok(8),
            bits => Err(LayoutError::UnsupportedPointerWidth(bits)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Niche {
    pub offset: u64,
    pub bits: u8,
    pub first: u128,
    pub count: u128,
}

impl Niche {
    fn shifted(&self, by: u64) -> Result<Self, LayoutError> {
        Ok(Self {
            offset: self.offset.checked_add(by).ok_or(LayoutError::SizeOverflow)?,
            bits: self.bits,
            first: self.first,
            count: self.count,
        })
    }

    fn consume_lowest(&self, count: usize) -> Option<(Vec<u128>, Option<Self>)> {
        let count = count as u128;
        if self.count < count {
            return None;
        }
        let values = (0..count).map(|index| self.first + index).collect();
        let remaining = self.count - count;
        let niche = (remaining != 0).then(|| Self {
            offset: self.offset,
            bits: self.bits,
            first: self.first + count,
            count: remaining,
        });
        Some((values, niche))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub declaration_index: u32,
    pub offset: u64,
    pub size: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantLayout {
    pub name: String,
    pub declaration_index: u32,
    pub fields: Vec<FieldLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagLayout {
    pub offset: u64,
    pub size: u8,
    pub variant_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumEncoding {
    /// One payload representation remains unchanged. Fieldless variants consume
    /// invalid bit patterns from that payload's stable niche.
    Niche {
        payload_variant: u32,
        niche_offset: u64,
        niche_bits: u8,
        fieldless_values: Vec<(u32, u128)>,
    },
    /// Ordinary explicit tag plus shared payload storage.
    Tagged {
        tag: TagLayout,
        payload_offset: u64,
    },
    /// A single variant needs no discriminator.
    Single,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutKind {
    Scalar,
    ZeroSized,
    Struct,
    Array { length: u64, stride: u64 },
    Slice { data_offset: u64, len_offset: u64 },
    Enum { tag: Option<TagLayout> },
    Optional { encoding: SumEncoding },
    Result { encoding: SumEncoding },
    Tagged {
        encoding: SumEncoding,
        variants: Vec<VariantLayout>,
    },
    Alias,
    Distinct,
    BitStruct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    /// Physical field order. Source identity remains available through
    /// `declaration_index`.
    pub fields: Vec<FieldLayout>,
    /// Stable invalid representations still available to enclosing types.
    pub niche: Option<Niche>,
    pub kind: LayoutKind,
}

impl Layout {
    fn scalar(size: u64, align: u64, niche: Option<Niche>) -> Self {
        Self {
            size,
            align,
            fields: Vec::new(),
            niche,
            kind: LayoutKind::Scalar,
        }
    }

    fn zero_sized() -> Self {
        Self {
            size: 0,
            align: 1,
            fields: Vec::new(),
            niche: None,
            kind: LayoutKind::ZeroSized,
        }
    }

    pub fn field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    UnsupportedPointerWidth(u16),
    UnsupportedType(&'static str),
    SemanticTypeLeak(&'static str),
    UnknownNominal(DefId),
    RecursiveType(Vec<DefId>),
    UnknownArrayLength,
    SizeOverflow,
    TooManyVariants,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPointerWidth(bits) => {
                write!(f, "unsupported Forge pointer width: {bits}")
            }
            Self::UnsupportedType(kind) => write!(f, "unsupported Forge layout type: {kind}"),
            Self::SemanticTypeLeak(kind) => {
                write!(f, "non-concrete semantic type reached layout: {kind}")
            }
            Self::UnknownNominal(id) => write!(f, "missing nominal type definition for {id:?}"),
            Self::RecursiveType(path) => write!(f, "recursive by-value type: {path:?}"),
            Self::UnknownArrayLength => write!(f, "array length is not compile-time known"),
            Self::SizeOverflow => write!(f, "Forge type layout exceeds addressable size"),
            Self::TooManyVariants => write!(f, "sum type has too many variants"),
        }
    }
}

impl Error for LayoutError {}

/// Single authoritative Forge memory-layout engine.
///
/// It owns Forge byte layout, alignment, physical field order, stable niches,
/// and sum representations. It does not know about Cranelift or any platform C
/// ABI. Call-ABI decomposition is a later layer built on these facts.
pub struct LayoutEngine<'a> {
    target: LayoutTarget,
    definitions: &'a TypeDefinitionTable,
    nominal_cache: BTreeMap<DefId, Layout>,
    active_nominals: Vec<DefId>,
}

impl<'a> LayoutEngine<'a> {
    pub fn new(target: LayoutTarget, definitions: &'a TypeDefinitionTable) -> Self {
        Self {
            target,
            definitions,
            nominal_cache: BTreeMap::new(),
            active_nominals: Vec::new(),
        }
    }

    pub const fn target(&self) -> LayoutTarget {
        self.target
    }

    pub fn layout_of(&mut self, ty: &Ty) -> Result<Layout, LayoutError> {
        match ty {
            Ty::Bool => Ok(Layout::scalar(
                1,
                1,
                Some(Niche {
                    offset: 0,
                    bits: 8,
                    first: 2,
                    count: 254,
                }),
            )),
            Ty::Byte => Ok(Layout::scalar(1, 1, None)),
            Ty::Char => Ok(Layout::scalar(4, 4, None)),
            Ty::Int { width, .. } => self.layout_integer(*width),
            Ty::Float { bits } => match bits {
                32 => Ok(Layout::scalar(4, 4, None)),
                64 => Ok(Layout::scalar(8, 8, None)),
                _ => Err(LayoutError::UnsupportedType("floating-point width")),
            },
            // Forge duration is a fixed-width scalar representation. Unit
            // semantics have already been resolved before FIR/layout.
            Ty::Duration => Ok(Layout::scalar(8, 8, None)),
            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {
                self.layout_pointer()
            }
            Ty::Void | Ty::Never => Ok(Layout::zero_sized()),
            Ty::Nominal(owner) => self.layout_nominal(*owner),
            Ty::Optional { inner } => self.layout_optional(inner),
            Ty::Slice { .. } => self.layout_slice(),
            Ty::Array { element, length } => {
                let length = length.ok_or(LayoutError::UnknownArrayLength)?;
                self.layout_array(element, length)
            }
            Ty::Result { ok, error } => self.layout_result(ok, error),

            Ty::Str => Err(LayoutError::UnsupportedType("unsized str")),
            Ty::ContextSlot { .. } => Err(LayoutError::UnsupportedType("context slot")),
            Ty::Closure { .. } => Err(LayoutError::UnsupportedType("closure")),

            Ty::Error => Err(LayoutError::SemanticTypeLeak("error")),
            Ty::Unknown => Err(LayoutError::SemanticTypeLeak("unknown")),
            Ty::IntLiteral => Err(LayoutError::SemanticTypeLeak("integer literal")),
            Ty::FloatLiteral => Err(LayoutError::SemanticTypeLeak("float literal")),
            Ty::NoneLiteral => Err(LayoutError::SemanticTypeLeak("none literal")),
        }
    }

    fn layout_integer(&self, width: IntWidth) -> Result<Layout, LayoutError> {
        let bytes = match width {
            IntWidth::W8 => 1,
            IntWidth::W16 => 2,
            IntWidth::W32 => 4,
            IntWidth::W64 => 8,
            IntWidth::Pointer => self.target.pointer_bytes()?,
        };
        Ok(Layout::scalar(bytes, bytes, None))
    }

    fn layout_pointer(&self) -> Result<Layout, LayoutError> {
        let bytes = self.target.pointer_bytes()?;
        Ok(Layout::scalar(
            bytes,
            bytes,
            Some(Niche {
                offset: 0,
                bits: self.target.pointer_bits as u8,
                first: 0,
                count: 1,
            }),
        ))
    }

    fn layout_slice(&self) -> Result<Layout, LayoutError> {
        let pointer = self.layout_pointer()?;
        let usize_layout = self.layout_integer(IntWidth::Pointer)?;
        let mut layout = place_fields(vec![
            FieldCandidate::new("data", 0, pointer),
            FieldCandidate::new("len", 1, usize_layout),
        ])?;
        let data_offset = layout.field("data").expect("slice data field").offset;
        let len_offset = layout.field("len").expect("slice len field").offset;
        // C9 deliberately does not expose a slice-level niche yet even though
        // its data pointer is itself non-null.
        layout.niche = None;
        layout.kind = LayoutKind::Slice {
            data_offset,
            len_offset,
        };
        Ok(layout)
    }

    fn layout_array(&mut self, element: &Ty, length: u64) -> Result<Layout, LayoutError> {
        let element = self.layout_of(element)?;
        let stride = round_up(element.size, element.align)?;
        let size = stride.checked_mul(length).ok_or(LayoutError::SizeOverflow)?;
        Ok(Layout {
            size,
            align: element.align,
            fields: Vec::new(),
            niche: None,
            kind: LayoutKind::Array { length, stride },
        })
    }

    fn layout_optional(&mut self, inner: &Ty) -> Result<Layout, LayoutError> {
        let payload = self.layout_of(inner)?;
        if let Some(niche) = payload.niche.clone() {
            if let Some((values, remaining)) = niche.consume_lowest(1) {
                let mut layout = payload;
                layout.niche = remaining;
                layout.kind = LayoutKind::Optional {
                    encoding: SumEncoding::Niche {
                        payload_variant: 1,
                        niche_offset: niche.offset,
                        niche_bits: niche.bits,
                        fieldless_values: vec![(0, values[0])],
                    },
                };
                return Ok(layout);
            }
        }

        let none = VariantCandidate::fieldless("None", 0);
        let some = VariantCandidate::single_payload("Some", 1, "value", payload);
        let mut layout = self.layout_sum(vec![none, some])?;
        let encoding = match &layout.kind {
            LayoutKind::Tagged { encoding, .. } => encoding.clone(),
            _ => unreachable!("layout_sum returns tagged representation"),
        };
        layout.kind = LayoutKind::Optional { encoding };
        Ok(layout)
    }

    fn layout_result(&mut self, ok: &Ty, error: &Ty) -> Result<Layout, LayoutError> {
        let ok = self.layout_of(ok)?;
        let error = self.layout_of(error)?;
        let variants = vec![
            VariantCandidate::single_payload("Ok", 0, "value", ok),
            VariantCandidate::single_payload("Err", 1, "error", error),
        ];
        let mut layout = self.layout_sum(variants)?;
        let encoding = match &layout.kind {
            LayoutKind::Tagged { encoding, .. } => encoding.clone(),
            _ => unreachable!("layout_sum returns tagged representation"),
        };
        layout.kind = LayoutKind::Result { encoding };
        Ok(layout)
    }

    fn layout_nominal(&mut self, owner: DefId) -> Result<Layout, LayoutError> {
        if let Some(layout) = self.nominal_cache.get(&owner) {
            return Ok(layout.clone());
        }
        if let Some(start) = self.active_nominals.iter().position(|active| *active == owner) {
            let mut cycle = self.active_nominals[start..].to_vec();
            cycle.push(owner);
            return Err(LayoutError::RecursiveType(cycle));
        }
        let definition = self
            .definitions
            .get(&owner)
            .ok_or(LayoutError::UnknownNominal(owner))?
            .clone();

        self.active_nominals.push(owner);
        let result = match definition.kind {
            TypeDefinitionKind::Alias { target } => {
                let mut layout = self.layout_of(&target)?;
                layout.kind = LayoutKind::Alias;
                Ok(layout)
            }
            TypeDefinitionKind::Distinct { underlying } => {
                let mut layout = self.layout_of(&underlying)?;
                layout.kind = LayoutKind::Distinct;
                Ok(layout)
            }
            TypeDefinitionKind::Struct { fields } => self.layout_struct(&fields),
            TypeDefinitionKind::Enum { variants } => self.layout_enum(variants.len()),
            TypeDefinitionKind::Tagged { variants } => self.layout_tagged(&variants),
            TypeDefinitionKind::BitStruct { storage } => {
                let mut layout = self.layout_of(&storage)?;
                layout.kind = LayoutKind::BitStruct;
                Ok(layout)
            }
        };
        self.active_nominals.pop();

        let layout = result?;
        self.nominal_cache.insert(owner, layout.clone());
        Ok(layout)
    }

    fn layout_struct(&mut self, fields: &[TypeFieldDefinition]) -> Result<Layout, LayoutError> {
        let mut candidates = Vec::with_capacity(fields.len());
        for field in fields {
            candidates.push(FieldCandidate::new(
                field.name.clone(),
                field.declaration_index,
                self.layout_of(&field.ty)?,
            ));
        }
        let mut layout = place_fields(candidates)?;
        layout.kind = LayoutKind::Struct;
        Ok(layout)
    }

    fn layout_enum(&self, variant_count: usize) -> Result<Layout, LayoutError> {
        if variant_count <= 1 {
            return Ok(Layout {
                kind: LayoutKind::Enum { tag: None },
                ..Layout::zero_sized()
            });
        }
        let tag_bytes = tag_bytes(variant_count)?;
        let bits = (tag_bytes * 8) as u8;
        let capacity = 1u128 << bits;
        let used = variant_count as u128;
        let niche = (capacity > used).then(|| Niche {
            offset: 0,
            bits,
            first: used,
            count: capacity - used,
        });
        Ok(Layout {
            size: tag_bytes,
            align: tag_bytes,
            fields: Vec::new(),
            niche,
            kind: LayoutKind::Enum {
                tag: Some(TagLayout {
                    offset: 0,
                    size: tag_bytes as u8,
                    variant_count: variant_count as u64,
                }),
            },
        })
    }

    fn layout_tagged(
        &mut self,
        variants: &[TypeVariantDefinition],
    ) -> Result<Layout, LayoutError> {
        let mut candidates = Vec::with_capacity(variants.len());
        for variant in variants {
            let mut fields = Vec::with_capacity(variant.fields.len());
            for field in &variant.fields {
                fields.push(FieldCandidate::new(
                    field.name.clone(),
                    field.declaration_index,
                    self.layout_of(&field.ty)?,
                ));
            }
            let mut payload = place_fields(fields)?;
            payload.kind = LayoutKind::Struct;
            candidates.push(VariantCandidate {
                name: variant.name.clone(),
                declaration_index: variant.declaration_index,
                payload,
            });
        }
        self.layout_sum(candidates)
    }

    fn layout_sum(&self, variants: Vec<VariantCandidate>) -> Result<Layout, LayoutError> {
        if variants.is_empty() {
            return Ok(Layout {
                kind: LayoutKind::Tagged {
                    encoding: SumEncoding::Single,
                    variants: Vec::new(),
                },
                ..Layout::zero_sized()
            });
        }
        if variants.len() == 1 {
            let only = &variants[0];
            return Ok(Layout {
                size: only.payload.size,
                align: only.payload.align,
                fields: only.payload.fields.clone(),
                niche: only.payload.niche.clone(),
                kind: LayoutKind::Tagged {
                    encoding: SumEncoding::Single,
                    variants: vec![only.variant_layout(0)?],
                },
            });
        }

        let nonzero = variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| variant.payload.size != 0)
            .collect::<Vec<_>>();
        if nonzero.len() == 1 {
            let (payload_index, payload_variant) = nonzero[0];
            let fieldless_count = variants.len() - 1;
            if let Some(niche) = &payload_variant.payload.niche {
                if let Some((values, remaining)) = niche.consume_lowest(fieldless_count) {
                    let mut value_index = 0usize;
                    let mut fieldless_values = Vec::with_capacity(fieldless_count);
                    for (index, _) in variants.iter().enumerate() {
                        if index == payload_index {
                            continue;
                        }
                        fieldless_values.push((index as u32, values[value_index]));
                        value_index += 1;
                    }
                    let variant_layouts = variants
                        .iter()
                        .map(|variant| variant.variant_layout(0))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Layout {
                        size: payload_variant.payload.size,
                        align: payload_variant.payload.align,
                        fields: payload_variant.payload.fields.clone(),
                        niche: remaining,
                        kind: LayoutKind::Tagged {
                            encoding: SumEncoding::Niche {
                                payload_variant: payload_index as u32,
                                niche_offset: niche.offset,
                                niche_bits: niche.bits,
                                fieldless_values,
                            },
                            variants: variant_layouts,
                        },
                    });
                }
            }
        }

        let tag_size = tag_bytes(variants.len())?;
        let tag_bits = (tag_size * 8) as u8;
        let capacity = 1u128 << tag_bits;
        let used = variants.len() as u128;
        let tag_niche = (capacity > used).then(|| Niche {
            offset: 0,
            bits: tag_bits,
            first: used,
            count: capacity - used,
        });
        let tag = Layout::scalar(tag_size, tag_size, tag_niche);

        let payload_size = variants
            .iter()
            .map(|variant| variant.payload.size)
            .max()
            .unwrap_or(0);
        let payload_align = variants
            .iter()
            .map(|variant| variant.payload.align)
            .max()
            .unwrap_or(1);
        let payload = Layout {
            size: payload_size,
            align: payload_align,
            fields: Vec::new(),
            niche: None,
            kind: LayoutKind::Struct,
        };

        let storage = place_fields(vec![
            FieldCandidate::new("$tag", 0, tag),
            FieldCandidate::new("$payload", 1, payload),
        ])?;
        let tag_offset = storage.field("$tag").expect("tag field").offset;
        let payload_offset = storage.field("$payload").expect("payload field").offset;
        let niche = (capacity > used).then(|| Niche {
            offset: tag_offset,
            bits: tag_bits,
            first: used,
            count: capacity - used,
        });
        let variant_layouts = variants
            .iter()
            .map(|variant| variant.variant_layout(payload_offset))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Layout {
            size: storage.size,
            align: storage.align,
            fields: storage.fields,
            niche,
            kind: LayoutKind::Tagged {
                encoding: SumEncoding::Tagged {
                    tag: TagLayout {
                        offset: tag_offset,
                        size: tag_size as u8,
                        variant_count: variants.len() as u64,
                    },
                    payload_offset,
                },
                variants: variant_layouts,
            },
        })
    }
}

#[derive(Debug, Clone)]
struct FieldCandidate {
    name: String,
    declaration_index: u32,
    layout: Layout,
}

impl FieldCandidate {
    fn new(name: impl Into<String>, declaration_index: u32, layout: Layout) -> Self {
        Self {
            name: name.into(),
            declaration_index,
            layout,
        }
    }
}

#[derive(Debug, Clone)]
struct VariantCandidate {
    name: String,
    declaration_index: u32,
    payload: Layout,
}

impl VariantCandidate {
    fn fieldless(name: impl Into<String>, declaration_index: u32) -> Self {
        Self {
            name: name.into(),
            declaration_index,
            payload: Layout::zero_sized(),
        }
    }

    fn single_payload(
        name: impl Into<String>,
        declaration_index: u32,
        field_name: impl Into<String>,
        payload: Layout,
    ) -> Self {
        let payload = if payload.size == 0 {
            Layout::zero_sized()
        } else {
            Layout {
                size: payload.size,
                align: payload.align,
                fields: vec![FieldLayout {
                    name: field_name.into(),
                    declaration_index: 0,
                    offset: 0,
                    size: payload.size,
                    align: payload.align,
                }],
                niche: payload.niche,
                kind: LayoutKind::Struct,
            }
        };
        Self {
            name: name.into(),
            declaration_index,
            payload,
        }
    }

    fn variant_layout(&self, payload_offset: u64) -> Result<VariantLayout, LayoutError> {
        let mut fields = self.payload.fields.clone();
        for field in &mut fields {
            field.offset = field
                .offset
                .checked_add(payload_offset)
                .ok_or(LayoutError::SizeOverflow)?;
        }
        Ok(VariantLayout {
            name: self.name.clone(),
            declaration_index: self.declaration_index,
            fields,
        })
    }
}

fn place_fields(mut fields: Vec<FieldCandidate>) -> Result<Layout, LayoutError> {
    fields.sort_by(|left, right| {
        let left_zero = left.layout.size == 0;
        let right_zero = right.layout.size == 0;
        match (left_zero, right_zero) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            (true, true) => left.declaration_index.cmp(&right.declaration_index),
            (false, false) => right
                .layout
                .align
                .cmp(&left.layout.align)
                .then_with(|| right.layout.size.cmp(&left.layout.size))
                .then_with(|| left.declaration_index.cmp(&right.declaration_index)),
        }
    });

    let aggregate_align = fields
        .iter()
        .filter(|field| field.layout.size != 0)
        .map(|field| field.layout.align)
        .max()
        .unwrap_or(1);
    let mut offset = 0u64;
    let mut placed = Vec::with_capacity(fields.len());
    let mut aggregate_niche = None;
    for field in fields {
        if field.layout.size == 0 {
            placed.push(FieldLayout {
                name: field.name,
                declaration_index: field.declaration_index,
                offset,
                size: 0,
                align: 1,
            });
            continue;
        }
        offset = round_up(offset, field.layout.align)?;
        if aggregate_niche.is_none() {
            if let Some(niche) = &field.layout.niche {
                aggregate_niche = Some(niche.shifted(offset)?);
            }
        }
        placed.push(FieldLayout {
            name: field.name,
            declaration_index: field.declaration_index,
            offset,
            size: field.layout.size,
            align: field.layout.align,
        });
        offset = offset
            .checked_add(field.layout.size)
            .ok_or(LayoutError::SizeOverflow)?;
    }
    let size = round_up(offset, aggregate_align)?;
    Ok(Layout {
        size,
        align: aggregate_align,
        fields: placed,
        niche: aggregate_niche,
        kind: LayoutKind::Struct,
    })
}

fn round_up(value: u64, align: u64) -> Result<u64, LayoutError> {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or(LayoutError::SizeOverflow)
}

fn tag_bytes(variant_count: usize) -> Result<u64, LayoutError> {
    let count = variant_count as u128;
    if count <= (1u128 << 8) {
        Ok(1)
    } else if count <= (1u128 << 16) {
        Ok(2)
    } else if count <= (1u128 << 32) {
        Ok(4)
    } else if count <= (1u128 << 64) {
        Ok(8)
    } else {
        Err(LayoutError::TooManyVariants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_frontend::{TypeDefinition, TypeDefinitionKind, TypeFieldDefinition, TypeVariantDefinition};

    fn u(width: IntWidth) -> Ty {
        Ty::Int {
            signed: false,
            width,
        }
    }

    fn reference() -> Ty {
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Byte),
        }
    }

    fn field(name: &str, index: u32, ty: Ty) -> TypeFieldDefinition {
        TypeFieldDefinition {
            name: name.into(),
            ty,
            declaration_index: index,
        }
    }

    fn variant(name: &str, index: u32, fields: Vec<TypeFieldDefinition>) -> TypeVariantDefinition {
        TypeVariantDefinition {
            name: name.into(),
            declaration_index: index,
            fields,
        }
    }

    fn definition(owner: u32, kind: TypeDefinitionKind) -> (DefId, TypeDefinition) {
        let owner = DefId(owner);
        (owner, TypeDefinition { owner, kind })
    }

    fn engine<'a>(defs: &'a TypeDefinitionTable) -> LayoutEngine<'a> {
        LayoutEngine::new(LayoutTarget::new(32), defs)
    }

    #[test]
    fn scalar_layouts_follow_target_width() {
        let defs = BTreeMap::new();
        let mut sia = engine(&defs);
        assert_eq!(sia.layout_of(&Ty::Bool).unwrap().size, 1);
        assert_eq!(sia.layout_of(&u(IntWidth::W16)).unwrap().align, 2);
        assert_eq!(sia.layout_of(&u(IntWidth::W64)).unwrap().size, 8);
        assert_eq!(sia.layout_of(&u(IntWidth::Pointer)).unwrap().size, 4);
        assert_eq!(sia.layout_of(&reference()).unwrap().size, 4);
        assert_eq!(sia.layout_of(&Ty::Float { bits: 32 }).unwrap().size, 4);
        assert_eq!(sia.layout_of(&Ty::Char).unwrap().size, 4);
        assert_eq!(sia.layout_of(&Ty::Duration).unwrap().size, 8);

        let mut wide = LayoutEngine::new(LayoutTarget::new(64), &defs);
        assert_eq!(wide.layout_of(&u(IntWidth::Pointer)).unwrap().size, 8);
        assert_eq!(wide.layout_of(&reference()).unwrap().align, 8);
    }

    #[test]
    fn invalid_pointer_width_is_rejected() {
        let defs = BTreeMap::new();
        let mut layout = LayoutEngine::new(LayoutTarget::new(48), &defs);
        assert_eq!(
            layout.layout_of(&u(IntWidth::Pointer)),
            Err(LayoutError::UnsupportedPointerWidth(48))
        );
    }

    #[test]
    fn bool_and_pointer_expose_stable_niches() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        assert_eq!(
            layout.layout_of(&Ty::Bool).unwrap().niche,
            Some(Niche {
                offset: 0,
                bits: 8,
                first: 2,
                count: 254,
            })
        );
        assert_eq!(
            layout.layout_of(&reference()).unwrap().niche,
            Some(Niche {
                offset: 0,
                bits: 32,
                first: 0,
                count: 1,
            })
        );
    }

    #[test]
    fn struct_fields_are_reordered_deterministically() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct {
                fields: vec![
                    field("a", 0, u(IntWidth::W8)),
                    field("b", 1, u(IntWidth::W64)),
                    field("c", 2, u(IntWidth::W16)),
                    field("d", 3, u(IntWidth::W32)),
                ],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!((value.size, value.align), (16, 8));
        assert_eq!(
            value
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            vec![("b", 0), ("d", 8), ("c", 12), ("a", 14)]
        );
    }

    #[test]
    fn declaration_index_breaks_equal_layout_ties() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct {
                fields: vec![
                    field("first", 0, u(IntWidth::W32)),
                    field("second", 1, u(IntWidth::W32)),
                ],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!(value.field("first").unwrap().offset, 0);
        assert_eq!(value.field("second").unwrap().offset, 4);
    }

    #[test]
    fn nested_structs_use_nested_size_and_alignment() {
        let defs = BTreeMap::from([
            definition(
                1,
                TypeDefinitionKind::Struct {
                    fields: vec![
                        field("small", 0, u(IntWidth::W8)),
                        field("word", 1, u(IntWidth::W32)),
                    ],
                },
            ),
            definition(
                2,
                TypeDefinitionKind::Struct {
                    fields: vec![
                        field("lead", 0, u(IntWidth::W8)),
                        field("inner", 1, Ty::Nominal(DefId(1))),
                        field("wide", 2, u(IntWidth::W64)),
                    ],
                },
            ),
        ]);
        let mut layout = engine(&defs);
        let inner = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!((inner.size, inner.align), (8, 4));
        let outer = layout.layout_of(&Ty::Nominal(DefId(2))).unwrap();
        assert_eq!((outer.size, outer.align), (24, 8));
        assert_eq!(outer.field("wide").unwrap().offset, 0);
        assert_eq!(outer.field("inner").unwrap().offset, 8);
        assert_eq!(outer.field("lead").unwrap().offset, 16);
    }

    #[test]
    fn struct_propagates_a_stable_child_niche() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct {
                fields: vec![field("ptr", 0, reference()), field("tag", 1, Ty::Byte)],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!(value.field("ptr").unwrap().offset, 0);
        assert_eq!(
            value.niche,
            Some(Niche {
                offset: 0,
                bits: 32,
                first: 0,
                count: 1,
            })
        );
    }

    #[test]
    fn arrays_use_element_stride_and_detect_unknown_length() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        let array = Ty::Array {
            element: Box::new(u(IntWidth::W16)),
            length: Some(3),
        };
        let result = layout.layout_of(&array).unwrap();
        assert_eq!((result.size, result.align), (6, 2));
        assert_eq!(result.kind, LayoutKind::Array { length: 3, stride: 2 });

        let unknown = Ty::Array {
            element: Box::new(Ty::Byte),
            length: None,
        };
        assert_eq!(layout.layout_of(&unknown), Err(LayoutError::UnknownArrayLength));
    }

    #[test]
    fn zero_sized_fields_sort_last_in_declaration_order() {
        let defs = BTreeMap::from([
            definition(1, TypeDefinitionKind::Struct { fields: vec![] }),
            definition(
                2,
                TypeDefinitionKind::Struct {
                    fields: vec![
                        field("marker_a", 0, Ty::Nominal(DefId(1))),
                        field("byte", 1, Ty::Byte),
                        field(
                            "zero_array",
                            2,
                            Ty::Array {
                                element: Box::new(u(IntWidth::W64)),
                                length: Some(0),
                            },
                        ),
                        field("marker_b", 3, Ty::Nominal(DefId(1))),
                    ],
                },
            ),
        ]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(2))).unwrap();
        assert_eq!((value.size, value.align), (1, 1));
        assert_eq!(
            value.fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(),
            vec!["byte", "marker_a", "zero_array", "marker_b"]
        );
        assert_eq!(value.field("marker_a").unwrap().offset, 1);
        assert_eq!(value.field("zero_array").unwrap().offset, 1);
        assert_eq!(value.field("zero_array").unwrap().align, 1);
    }

    #[test]
    fn arrays_of_zsts_remain_zero_sized() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct { fields: vec![] },
        )]);
        let mut layout = engine(&defs);
        let array = Ty::Array {
            element: Box::new(Ty::Nominal(DefId(1))),
            length: Some(100),
        };
        let value = layout.layout_of(&array).unwrap();
        assert_eq!(value.size, 0);
        assert_eq!(value.kind, LayoutKind::Array { length: 100, stride: 0 });
    }

    #[test]
    fn option_reference_consumes_null_niche_without_growing() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        let optional = Ty::Optional {
            inner: Box::new(reference()),
        };
        let value = layout.layout_of(&optional).unwrap();
        assert_eq!((value.size, value.align), (4, 4));
        assert_eq!(value.niche, None);
        assert!(matches!(
            value.kind,
            LayoutKind::Optional {
                encoding: SumEncoding::Niche {
                    payload_variant: 1,
                    niche_offset: 0,
                    niche_bits: 32,
                    ref fieldless_values,
                }
            } if fieldless_values == &vec![(0, 0)]
        ));
    }

    #[test]
    fn nested_option_bool_consumes_niches_in_numeric_order() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        let once = Ty::Optional {
            inner: Box::new(Ty::Bool),
        };
        let once_layout = layout.layout_of(&once).unwrap();
        assert_eq!((once_layout.size, once_layout.align), (1, 1));
        assert_eq!(once_layout.niche.as_ref().unwrap().first, 3);
        assert_eq!(once_layout.niche.as_ref().unwrap().count, 253);

        let twice = Ty::Optional {
            inner: Box::new(once),
        };
        let twice_layout = layout.layout_of(&twice).unwrap();
        assert_eq!(twice_layout.size, 1);
        assert_eq!(twice_layout.niche.as_ref().unwrap().first, 4);
    }

    #[test]
    fn result_with_two_payloads_uses_smallest_explicit_tag() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        let result = Ty::Result {
            ok: Box::new(u(IntWidth::W32)),
            error: Box::new(u(IntWidth::W32)),
        };
        let value = layout.layout_of(&result).unwrap();
        assert_eq!((value.size, value.align), (8, 4));
        assert!(matches!(
            value.kind,
            LayoutKind::Result {
                encoding: SumEncoding::Tagged {
                    tag: TagLayout { size: 1, offset: 4, variant_count: 2 },
                    payload_offset: 0,
                }
            }
        ));
        assert_eq!(value.niche.as_ref().unwrap().offset, 4);
        assert_eq!(value.niche.as_ref().unwrap().first, 2);
    }

    #[test]
    fn result_with_one_zst_payload_uses_other_payload_niche() {
        let defs = BTreeMap::new();
        let mut layout = engine(&defs);
        let result = Ty::Result {
            ok: Box::new(Ty::Void),
            error: Box::new(reference()),
        };
        let value = layout.layout_of(&result).unwrap();
        assert_eq!((value.size, value.align), (4, 4));
        assert!(matches!(
            value.kind,
            LayoutKind::Result {
                encoding: SumEncoding::Niche {
                    payload_variant: 1,
                    niche_offset: 0,
                    niche_bits: 32,
                    ..
                }
            }
        ));
    }

    #[test]
    fn simple_nominal_tagged_union_uses_payload_niche() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Tagged {
                variants: vec![
                    variant("None", 0, vec![]),
                    variant("Some", 1, vec![field("value", 0, reference())]),
                ],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!((value.size, value.align), (4, 4));
        assert!(matches!(
            value.kind,
            LayoutKind::Tagged {
                encoding: SumEncoding::Niche {
                    payload_variant: 1,
                    niche_offset: 0,
                    niche_bits: 32,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn explicit_tagged_union_reports_variant_field_offsets() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Tagged {
                variants: vec![
                    variant("A", 0, vec![field("a", 0, u(IntWidth::W32))]),
                    variant("B", 1, vec![field("b", 0, u(IntWidth::W64))]),
                ],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!((value.size, value.align), (16, 8));
        let LayoutKind::Tagged { encoding, variants } = &value.kind else {
            panic!("expected tagged layout");
        };
        assert!(matches!(
            encoding,
            SumEncoding::Tagged {
                tag: TagLayout { offset: 8, size: 1, variant_count: 2 },
                payload_offset: 0,
            }
        ));
        assert_eq!(variants[0].fields[0].offset, 0);
        assert_eq!(variants[1].fields[0].offset, 0);
    }

    #[test]
    fn enum_discriminant_width_grows_only_when_needed() {
        let variants_256 = (0..256)
            .map(|index| variant(&format!("V{index}"), index, vec![]))
            .collect();
        let variants_257 = (0..257)
            .map(|index| variant(&format!("V{index}"), index, vec![]))
            .collect();
        let defs = BTreeMap::from([
            definition(1, TypeDefinitionKind::Enum { variants: variants_256 }),
            definition(2, TypeDefinitionKind::Enum { variants: variants_257 }),
        ]);
        let mut layout = engine(&defs);
        assert_eq!(layout.layout_of(&Ty::Nominal(DefId(1))).unwrap().size, 1);
        assert_eq!(layout.layout_of(&Ty::Nominal(DefId(2))).unwrap().size, 2);
    }

    #[test]
    fn enum_unused_discriminants_are_stable_niches() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Enum {
                variants: vec![variant("A", 0, vec![]), variant("B", 1, vec![])],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!(
            value.niche,
            Some(Niche {
                offset: 0,
                bits: 8,
                first: 2,
                count: 254,
            })
        );
    }

    #[test]
    fn aliases_and_distinct_types_preserve_representation() {
        let defs = BTreeMap::from([
            definition(
                1,
                TypeDefinitionKind::Alias {
                    target: reference(),
                },
            ),
            definition(
                2,
                TypeDefinitionKind::Distinct {
                    underlying: Ty::Nominal(DefId(1)),
                },
            ),
        ]);
        let mut layout = engine(&defs);
        let alias = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        let distinct = layout.layout_of(&Ty::Nominal(DefId(2))).unwrap();
        assert_eq!((alias.size, alias.align, alias.niche.clone()), (4, 4, distinct.niche.clone()));
        assert_eq!(distinct.kind, LayoutKind::Distinct);
    }

    #[test]
    fn direct_recursive_struct_is_rejected() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct {
                fields: vec![field("self", 0, Ty::Nominal(DefId(1)))],
            },
        )]);
        let mut layout = engine(&defs);
        assert_eq!(
            layout.layout_of(&Ty::Nominal(DefId(1))),
            Err(LayoutError::RecursiveType(vec![DefId(1), DefId(1)]))
        );
    }

    #[test]
    fn mutual_by_value_recursion_is_rejected() {
        let defs = BTreeMap::from([
            definition(
                1,
                TypeDefinitionKind::Struct {
                    fields: vec![field("b", 0, Ty::Nominal(DefId(2)))],
                },
            ),
            definition(
                2,
                TypeDefinitionKind::Struct {
                    fields: vec![field("a", 0, Ty::Nominal(DefId(1)))],
                },
            ),
        ]);
        let mut layout = engine(&defs);
        assert_eq!(
            layout.layout_of(&Ty::Nominal(DefId(1))),
            Err(LayoutError::RecursiveType(vec![DefId(1), DefId(2), DefId(1)]))
        );
    }

    #[test]
    fn recursion_through_pointer_is_finite() {
        let defs = BTreeMap::from([definition(
            1,
            TypeDefinitionKind::Struct {
                fields: vec![field(
                    "next",
                    0,
                    Ty::Pointer {
                        volatile: false,
                        inner: Box::new(Ty::Nominal(DefId(1))),
                    },
                )],
            },
        )]);
        let mut layout = engine(&defs);
        let value = layout.layout_of(&Ty::Nominal(DefId(1))).unwrap();
        assert_eq!((value.size, value.align), (4, 4));
    }

    #[test]
    fn alias_cycles_are_rejected() {
        let defs = BTreeMap::from([
            definition(
                1,
                TypeDefinitionKind::Alias {
                    target: Ty::Nominal(DefId(2)),
                },
            ),
            definition(
                2,
                TypeDefinitionKind::Alias {
                    target: Ty::Nominal(DefId(1)),
                },
            ),
        ]);
        let mut layout = engine(&defs);
        assert!(matches!(
            layout.layout_of(&Ty::Nominal(DefId(1))),
            Err(LayoutError::RecursiveType(_))
        ));
    }
}
