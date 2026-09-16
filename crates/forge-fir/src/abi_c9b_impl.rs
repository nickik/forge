use std::fmt;

use forge_frontend::{DefId, IntWidth, Ty, TypeDefinitionKind, TypeDefinitionTable};

use crate::layout::{Layout, LayoutEngine, LayoutError, LayoutKind, LayoutTarget, SumEncoding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbiTarget {
    pub word_bits: u16,
    pub pointer_bits: u16,
    pub direct_value_words: u8,
}

impl AbiTarget {
    pub const fn sia32() -> Self {
        Self {
            word_bits: 32,
            pointer_bits: 32,
            direct_value_words: 4,
        }
    }

    pub const fn native64() -> Self {
        Self {
            word_bits: 64,
            pointer_bits: 64,
            direct_value_words: 4,
        }
    }

    const fn word_bytes(self) -> u64 {
        (self.word_bits / 8) as u64
    }

    const fn direct_bits(self) -> u64 {
        self.word_bits as u64 * self.direct_value_words as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiPieceKind {
    Integer,
    Pointer,
}

/// Mapping from a byte-addressed Forge representation into one ABI scalar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiFragment {
    pub source_offset: u64,
    pub bits: u16,
    pub piece_bit_offset: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiPiece {
    pub kind: AbiPieceKind,
    /// Scalar width presented to the target ABI. Integer pieces are widened to
    /// one full ABI word; pointers retain pointer width.
    pub bits: u16,
    pub used_bits: u16,
    pub fragments: Vec<AbiFragment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiPassing {
    Direct,
    Indirect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiDecomposition {
    pub passing: AbiPassing,
    pub size: u64,
    pub align: u64,
    /// Indirect values have no direct pieces; the later call ABI supplies the
    /// pointer to caller-owned value storage.
    pub pieces: Vec<AbiPiece>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiError {
    Layout(LayoutError),
    UnsupportedTarget(&'static str),
    UnsupportedType(&'static str),
    MissingFieldType { owner: DefId, field: String },
}

impl From<LayoutError> for AbiError {
    fn from(value: LayoutError) -> Self {
        Self::Layout(value)
    }
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => write!(f, "{error}"),
            Self::UnsupportedTarget(reason) => write!(f, "unsupported Forge ABI target: {reason}"),
            Self::UnsupportedType(kind) => write!(f, "unsupported Forge ABI type: {kind}"),
            Self::MissingFieldType { owner, field } => {
                write!(f, "missing type for field `{field}` of {owner:?}")
            }
        }
    }
}

impl std::error::Error for AbiError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawKind {
    Integer,
    Pointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawFragment {
    kind: RawKind,
    source_offset: u64,
    bits: u16,
}

#[derive(Debug)]
struct RawCollector {
    fragments: Vec<RawFragment>,
    semantic_bits: u64,
    budget_bits: u64,
    overflow: bool,
}

impl RawCollector {
    fn new(target: AbiTarget) -> Self {
        Self {
            fragments: Vec::new(),
            semantic_bits: 0,
            budget_bits: target.direct_bits(),
            overflow: false,
        }
    }

    fn push(&mut self, fragment: RawFragment) {
        if self.overflow || fragment.bits == 0 {
            return;
        }
        self.semantic_bits = self.semantic_bits.saturating_add(fragment.bits as u64);
        if self.semantic_bits > self.budget_bits {
            self.overflow = true;
            return;
        }
        self.fragments.push(fragment);
    }
}

/// Forge-owned ABI decomposition. No Cranelift or C ABI policy belongs here.
pub struct AbiDecomposer<'a> {
    target: AbiTarget,
    definitions: &'a TypeDefinitionTable,
    layouts: LayoutEngine<'a>,
}

impl<'a> AbiDecomposer<'a> {
    pub fn new(target: AbiTarget, definitions: &'a TypeDefinitionTable) -> Result<Self, AbiError> {
        if target.word_bits == 0 || !target.word_bits.is_multiple_of(8) {
            return Err(AbiError::UnsupportedTarget(
                "ABI word must be a non-zero whole number of bytes",
            ));
        }
        if target.pointer_bits != target.word_bits {
            return Err(AbiError::UnsupportedTarget(
                "C9b requires pointer width to equal ABI word width",
            ));
        }
        Ok(Self {
            target,
            definitions,
            layouts: LayoutEngine::new(LayoutTarget::new(target.pointer_bits), definitions),
        })
    }

    pub const fn target(&self) -> AbiTarget {
        self.target
    }

    pub fn decompose(&mut self, ty: &Ty) -> Result<AbiDecomposition, AbiError> {
        let layout = self.layouts.layout_of(ty)?;
        if layout.size == 0 {
            return Ok(self.direct(&layout, Vec::new()));
        }

        let mut raw = RawCollector::new(self.target);
        self.collect_type(ty, 0, &layout, &mut raw)?;
        if raw.overflow {
            return Ok(self.indirect(&layout));
        }
        raw.fragments.sort_by_key(|fragment| fragment.source_offset);
        let pieces = coalesce(self.target, &raw.fragments)?;
        if pieces.len() > self.target.direct_value_words as usize {
            Ok(self.indirect(&layout))
        } else {
            Ok(self.direct(&layout, pieces))
        }
    }

    fn direct(&self, layout: &Layout, pieces: Vec<AbiPiece>) -> AbiDecomposition {
        AbiDecomposition {
            passing: AbiPassing::Direct,
            size: layout.size,
            align: layout.align,
            pieces,
        }
    }

    fn indirect(&self, layout: &Layout) -> AbiDecomposition {
        AbiDecomposition {
            passing: AbiPassing::Indirect,
            size: layout.size,
            align: layout.align,
            pieces: Vec::new(),
        }
    }

    fn collect_type(
        &mut self,
        ty: &Ty,
        base: u64,
        layout: &Layout,
        out: &mut RawCollector,
    ) -> Result<(), AbiError> {
        if out.overflow || layout.size == 0 {
            return Ok(());
        }
        match ty {
            Ty::Bool | Ty::Byte => out.push(integer(base, 8)),
            Ty::Char => out.push(integer(base, 32)),
            Ty::Int { width, .. } => out.push(integer(
                base,
                match width {
                    IntWidth::W8 => 8,
                    IntWidth::W16 => 16,
                    IntWidth::W32 => 32,
                    IntWidth::W64 => 64,
                    IntWidth::Pointer => self.target.word_bits,
                },
            )),
            Ty::Duration => out.push(integer(base, 64)),
            Ty::Pointer { .. }
            | Ty::Reference { .. }
            | Ty::Function { .. }
            | Ty::Closure { .. } => {
                out.push(pointer(base, self.target.pointer_bits));
            }
            Ty::Nominal(owner) => self.collect_nominal(*owner, base, layout, out)?,
            Ty::Array { element, length } => {
                let length = length.ok_or(LayoutError::UnknownArrayLength)?;
                let LayoutKind::Array { stride, .. } = &layout.kind else {
                    return Err(AbiError::UnsupportedType("array layout shape"));
                };
                let element_layout = self.layouts.layout_of(element)?;
                for index in 0..length {
                    if out.overflow {
                        break;
                    }
                    let offset = index
                        .checked_mul(*stride)
                        .and_then(|offset| base.checked_add(offset))
                        .ok_or(LayoutError::SizeOverflow)?;
                    self.collect_type(element, offset, &element_layout, out)?;
                }
            }
            Ty::Str => {
                let LayoutKind::Str {
                    data_offset,
                    len_offset,
                } = &layout.kind
                else {
                    return Err(AbiError::UnsupportedType("str layout shape"));
                };
                out.push(pointer(base + *data_offset, self.target.pointer_bits));
                out.push(integer(base + *len_offset, self.target.word_bits));
            }
            Ty::Slice { .. } => {
                let LayoutKind::Slice {
                    data_offset,
                    len_offset,
                } = &layout.kind
                else {
                    return Err(AbiError::UnsupportedType("slice layout shape"));
                };
                out.push(pointer(base + *data_offset, self.target.pointer_bits));
                out.push(integer(base + *len_offset, self.target.word_bits));
            }
            Ty::Optional { inner } => {
                let LayoutKind::Optional { encoding } = &layout.kind else {
                    return Err(AbiError::UnsupportedType("optional layout shape"));
                };
                match encoding {
                    SumEncoding::Niche { .. } | SumEncoding::Single => {
                        let child = self.layouts.layout_of(inner)?;
                        self.collect_type(inner, base, &child, out)?;
                    }
                    SumEncoding::Tagged {
                        tag,
                        payload_offset,
                    } => {
                        let child = self.layouts.layout_of(inner)?;
                        self.collect_type(inner, base + *payload_offset, &child, out)?;
                        out.push(integer(base + tag.offset, tag.size as u16 * 8));
                    }
                }
            }
            Ty::Result { ok, error } => {
                let LayoutKind::Result { encoding } = &layout.kind else {
                    return Err(AbiError::UnsupportedType("result layout shape"));
                };
                match encoding {
                    SumEncoding::Niche {
                        payload_variant, ..
                    } => {
                        let payload = if *payload_variant == 0 { ok } else { error };
                        let child = self.layouts.layout_of(payload)?;
                        self.collect_type(payload, base, &child, out)?;
                    }
                    SumEncoding::Tagged {
                        tag,
                        payload_offset,
                    } => {
                        let alternatives = vec![
                            self.collect_alternative(ok, base + *payload_offset)?,
                            self.collect_alternative(error, base + *payload_offset)?,
                        ];
                        self.push_merged_alternatives(alternatives, out);
                        out.push(integer(base + tag.offset, tag.size as u16 * 8));
                    }
                    SumEncoding::Single => {}
                }
            }
            Ty::Void | Ty::Never => {}
            Ty::Float { .. } => {
                return Err(AbiError::UnsupportedType(
                    "floating-point ABI classes are deferred",
                ));
            }
            Ty::ContextSlot { .. } => return Err(AbiError::UnsupportedType("context slot")),
            Ty::Error | Ty::Unknown | Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral => {
                return Err(AbiError::UnsupportedType("non-concrete semantic type"));
            }
        }
        Ok(())
    }

    fn collect_nominal(
        &mut self,
        owner: DefId,
        base: u64,
        layout: &Layout,
        out: &mut RawCollector,
    ) -> Result<(), AbiError> {
        let definition = self
            .definitions
            .get(&owner)
            .ok_or(LayoutError::UnknownNominal(owner))?
            .clone();
        match definition.kind {
            TypeDefinitionKind::Alias { target }
            | TypeDefinitionKind::Distinct { underlying: target }
            | TypeDefinitionKind::BitStruct { storage: target } => {
                let child = self.layouts.layout_of(&target)?;
                self.collect_type(&target, base, &child, out)?;
            }
            TypeDefinitionKind::Struct { fields } => {
                self.collect_struct_fields(owner, &fields, base, &layout.fields, out)?;
            }
            TypeDefinitionKind::Enum { variants } => {
                if variants.len() > 1 {
                    let LayoutKind::Enum { tag } = &layout.kind else {
                        return Err(AbiError::UnsupportedType("enum layout shape"));
                    };
                    if let Some(tag) = tag {
                        out.push(integer(base + tag.offset, tag.size as u16 * 8));
                    }
                }
            }
            TypeDefinitionKind::Tagged { variants } => {
                let LayoutKind::Tagged {
                    encoding,
                    variants: placed_variants,
                } = &layout.kind
                else {
                    return Err(AbiError::UnsupportedType("tagged layout shape"));
                };
                match encoding {
                    SumEncoding::Niche {
                        payload_variant, ..
                    } => {
                        let index = *payload_variant as usize;
                        let variant = variants
                            .get(index)
                            .ok_or(AbiError::UnsupportedType("niche payload variant"))?;
                        let placed = placed_variants
                            .get(index)
                            .ok_or(AbiError::UnsupportedType("niche variant layout"))?;
                        self.collect_struct_fields(
                            owner,
                            &variant.fields,
                            base,
                            &placed.fields,
                            out,
                        )?;
                    }
                    SumEncoding::Tagged {
                        tag,
                        payload_offset: _,
                    } => {
                        let mut alternatives = Vec::with_capacity(variants.len());
                        for (variant, placed) in variants.iter().zip(placed_variants) {
                            let mut collector = RawCollector::new(self.target);
                            self.collect_struct_fields(
                                owner,
                                &variant.fields,
                                base,
                                &placed.fields,
                                &mut collector,
                            )?;
                            alternatives.push(collector);
                        }
                        self.push_merged_alternatives(alternatives, out);
                        out.push(integer(base + tag.offset, tag.size as u16 * 8));
                    }
                    SumEncoding::Single => {
                        if let (Some(variant), Some(placed)) =
                            (variants.first(), placed_variants.first())
                        {
                            self.collect_struct_fields(
                                owner,
                                &variant.fields,
                                base,
                                &placed.fields,
                                out,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_struct_fields(
        &mut self,
        owner: DefId,
        fields: &[forge_frontend::TypeFieldDefinition],
        base: u64,
        placed_fields: &[crate::layout::FieldLayout],
        out: &mut RawCollector,
    ) -> Result<(), AbiError> {
        for placed in placed_fields {
            if out.overflow || placed.size == 0 || placed.name.starts_with('$') {
                continue;
            }
            let field = fields
                .iter()
                .find(|field| {
                    field.declaration_index == placed.declaration_index && field.name == placed.name
                })
                .ok_or_else(|| AbiError::MissingFieldType {
                    owner,
                    field: placed.name.clone(),
                })?;
            let child = self.layouts.layout_of(&field.ty)?;
            self.collect_type(&field.ty, base + placed.offset, &child, out)?;
        }
        Ok(())
    }

    fn collect_alternative(&mut self, ty: &Ty, base: u64) -> Result<RawCollector, AbiError> {
        let layout = self.layouts.layout_of(ty)?;
        let mut collector = RawCollector::new(self.target);
        self.collect_type(ty, base, &layout, &mut collector)?;
        Ok(collector)
    }

    fn push_merged_alternatives(&self, alternatives: Vec<RawCollector>, out: &mut RawCollector) {
        if alternatives.iter().any(|alternative| alternative.overflow) {
            out.overflow = true;
            return;
        }
        let variants = alternatives
            .into_iter()
            .map(|mut alternative| {
                alternative
                    .fragments
                    .sort_by_key(|fragment| (fragment.source_offset, fragment.bits));
                alternative.fragments
            })
            .collect::<Vec<_>>();
        if variants.windows(2).all(|pair| pair[0] == pair[1]) {
            if let Some(common) = variants.first() {
                for fragment in common {
                    out.push(*fragment);
                }
            }
            return;
        }

        // Incompatible union variants use an opaque integer envelope. Only
        // bytes occupied by at least one real fragment are covered: padding is
        // deliberately absent from the call ABI.
        let mut intervals = variants
            .iter()
            .flatten()
            .map(|fragment| {
                let bytes = u64::from(fragment.bits).div_ceil(8);
                (fragment.source_offset, fragment.source_offset + bytes)
            })
            .collect::<Vec<_>>();
        intervals.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::new();
        for (start, end) in intervals {
            match merged.last_mut() {
                Some((_, previous_end)) if start <= *previous_end => {
                    *previous_end = (*previous_end).max(end);
                }
                _ => merged.push((start, end)),
            }
        }
        for (start, end) in merged {
            let mut offset = start;
            while offset < end && !out.overflow {
                let bytes = (end - offset).min(self.target.word_bytes());
                out.push(integer(offset, (bytes * 8) as u16));
                offset += bytes;
            }
        }
    }
}

fn integer(source_offset: u64, bits: u16) -> RawFragment {
    RawFragment {
        kind: RawKind::Integer,
        source_offset,
        bits,
    }
}

fn pointer(source_offset: u64, bits: u16) -> RawFragment {
    RawFragment {
        kind: RawKind::Pointer,
        source_offset,
        bits,
    }
}

fn coalesce(target: AbiTarget, raw: &[RawFragment]) -> Result<Vec<AbiPiece>, AbiError> {
    let mut pieces = Vec::new();
    let mut integer_piece: Option<AbiPiece> = None;

    fn flush(pieces: &mut Vec<AbiPiece>, current: &mut Option<AbiPiece>) {
        if let Some(piece) = current.take() {
            pieces.push(piece);
        }
    }

    for fragment in raw {
        match fragment.kind {
            RawKind::Pointer => {
                flush(&mut pieces, &mut integer_piece);
                if fragment.bits != target.pointer_bits {
                    return Err(AbiError::UnsupportedTarget(
                        "pointer fragment width mismatch",
                    ));
                }
                pieces.push(AbiPiece {
                    kind: AbiPieceKind::Pointer,
                    bits: target.pointer_bits,
                    used_bits: target.pointer_bits,
                    fragments: vec![AbiFragment {
                        source_offset: fragment.source_offset,
                        bits: fragment.bits,
                        piece_bit_offset: 0,
                    }],
                });
            }
            RawKind::Integer => {
                let mut remaining = fragment.bits;
                let mut consumed = 0u16;
                while remaining != 0 {
                    let used = integer_piece.as_ref().map_or(0, |piece| piece.used_bits);
                    if used == target.word_bits {
                        flush(&mut pieces, &mut integer_piece);
                        continue;
                    }
                    let take = remaining.min(target.word_bits - used);
                    let piece = integer_piece.get_or_insert_with(|| AbiPiece {
                        kind: AbiPieceKind::Integer,
                        bits: target.word_bits,
                        used_bits: 0,
                        fragments: Vec::new(),
                    });
                    piece.fragments.push(AbiFragment {
                        source_offset: fragment.source_offset + u64::from(consumed / 8),
                        bits: take,
                        piece_bit_offset: piece.used_bits,
                    });
                    piece.used_bits += take;
                    remaining -= take;
                    consumed += take;
                    if piece.used_bits == target.word_bits {
                        flush(&mut pieces, &mut integer_piece);
                    }
                }
            }
        }
    }
    flush(&mut pieces, &mut integer_piece);
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use forge_frontend::{TypeDefinition, TypeFieldDefinition, TypeVariantDefinition};

    use super::*;

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
            declaration_index: index,
            ty,
        }
    }

    fn struct_def(owner: u32, fields: Vec<TypeFieldDefinition>) -> (DefId, TypeDefinition) {
        let owner = DefId(owner);
        (
            owner,
            TypeDefinition {
                owner,
                kind: TypeDefinitionKind::Struct { fields },
            },
        )
    }

    fn tagged_def(owner: u32, variants: Vec<TypeVariantDefinition>) -> (DefId, TypeDefinition) {
        let owner = DefId(owner);
        (
            owner,
            TypeDefinition {
                owner,
                kind: TypeDefinitionKind::Tagged { variants },
            },
        )
    }

    fn variant(name: &str, index: u32, fields: Vec<TypeFieldDefinition>) -> TypeVariantDefinition {
        TypeVariantDefinition {
            name: name.into(),
            declaration_index: index,
            fields,
        }
    }

    fn decompose(defs: &TypeDefinitionTable, ty: &Ty) -> AbiDecomposition {
        AbiDecomposer::new(AbiTarget::sia32(), defs)
            .unwrap()
            .decompose(ty)
            .unwrap()
    }

    #[test]
    fn requested_subword_examples() {
        let defs = BTreeMap::from([
            struct_def(
                1,
                vec![
                    field("a", 0, u(IntWidth::W8)),
                    field("b", 1, u(IntWidth::W8)),
                    field("c", 2, u(IntWidth::W8)),
                    field("d", 3, u(IntWidth::W8)),
                ],
            ),
            struct_def(
                2,
                vec![
                    field("a", 0, u(IntWidth::W16)),
                    field("b", 1, u(IntWidth::W8)),
                    field("c", 2, u(IntWidth::W8)),
                ],
            ),
            struct_def(
                3,
                vec![
                    field("a", 0, u(IntWidth::W32)),
                    field("b", 1, u(IntWidth::W8)),
                    field("c", 2, u(IntWidth::W8)),
                ],
            ),
        ]);
        assert_eq!(decompose(&defs, &Ty::Nominal(DefId(1))).pieces.len(), 1);
        assert_eq!(decompose(&defs, &Ty::Nominal(DefId(2))).pieces.len(), 1);
        assert_eq!(decompose(&defs, &Ty::Nominal(DefId(3))).pieces.len(), 2);
    }

    #[test]
    fn pointer_and_usize_are_two_pieces() {
        let defs = BTreeMap::from([struct_def(
            1,
            vec![
                field("ptr", 0, reference()),
                field("len", 1, u(IntWidth::Pointer)),
            ],
        )]);
        let value = decompose(&defs, &Ty::Nominal(DefId(1)));
        assert_eq!(value.pieces.len(), 2);
        assert_eq!(value.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(value.pieces[1].kind, AbiPieceKind::Integer);
    }

    #[test]
    fn option_reference_is_one_pointer_piece() {
        let defs = BTreeMap::new();
        let value = decompose(
            &defs,
            &Ty::Optional {
                inner: Box::new(reference()),
            },
        );
        assert_eq!(value.passing, AbiPassing::Direct);
        assert_eq!(value.pieces.len(), 1);
        assert_eq!(value.pieces[0].kind, AbiPieceKind::Pointer);
    }

    #[test]
    fn four_words_direct_five_words_indirect() {
        let defs = BTreeMap::new();
        let four = decompose(
            &defs,
            &Ty::Array {
                element: Box::new(u(IntWidth::W32)),
                length: Some(4),
            },
        );
        let five = decompose(
            &defs,
            &Ty::Array {
                element: Box::new(u(IntWidth::W32)),
                length: Some(5),
            },
        );
        assert_eq!((four.passing, four.pieces.len()), (AbiPassing::Direct, 4));
        assert_eq!(five.passing, AbiPassing::Indirect);
        assert!(five.pieces.is_empty());
    }

    #[test]
    fn zst_has_zero_pieces() {
        let defs = BTreeMap::from([struct_def(1, vec![])]);
        let value = decompose(&defs, &Ty::Nominal(DefId(1)));
        assert_eq!(value.size, 0);
        assert!(value.pieces.is_empty());
    }

    #[test]
    fn sixteen_bytes_coalesce_to_four_words_but_seventeen_go_indirect() {
        let defs = BTreeMap::from([
            struct_def(
                1,
                (0u32..16)
                    .map(|index| field(&format!("b{index}"), index, u(IntWidth::W8)))
                    .collect(),
            ),
            struct_def(
                2,
                (0u32..17)
                    .map(|index| field(&format!("b{index}"), index, u(IntWidth::W8)))
                    .collect(),
            ),
        ]);
        let sixteen = decompose(&defs, &Ty::Nominal(DefId(1)));
        let seventeen = decompose(&defs, &Ty::Nominal(DefId(2)));
        assert_eq!(sixteen.pieces.len(), 4);
        assert_eq!(seventeen.passing, AbiPassing::Indirect);
    }

    #[test]
    fn u64_is_two_ordinary_words_with_no_even_slot_rule() {
        let defs = BTreeMap::new();
        let value = decompose(&defs, &u(IntWidth::W64));
        assert_eq!(value.pieces.len(), 2);
        assert_eq!(value.pieces[0].fragments[0].source_offset, 0);
        assert_eq!(value.pieces[1].fragments[0].source_offset, 4);
    }

    #[test]
    fn pointer_breaks_integer_coalescing() {
        let defs = BTreeMap::from([struct_def(
            1,
            vec![
                field("a", 0, u(IntWidth::W8)),
                field("ptr", 1, reference()),
                field("b", 2, u(IntWidth::W8)),
            ],
        )]);
        let value = decompose(&defs, &Ty::Nominal(DefId(1)));
        // Forge layout puts the pointer first, then both bytes.
        assert_eq!(value.pieces.len(), 2);
        assert_eq!(value.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(value.pieces[1].used_bits, 16);
    }

    #[test]
    fn slice_is_pointer_plus_usize() {
        let defs = BTreeMap::new();
        let value = decompose(
            &defs,
            &Ty::Slice {
                mutable: false,
                element: Box::new(Ty::Byte),
            },
        );
        assert_eq!(value.pieces.len(), 2);
        assert_eq!(value.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(value.pieces[1].kind, AbiPieceKind::Integer);
    }

    #[test]
    fn explicit_sum_does_not_turn_tail_padding_into_abi_data() {
        let defs = BTreeMap::from([tagged_def(
            1,
            vec![
                variant(
                    "WideAndByte",
                    0,
                    vec![
                        field("wide", 0, u(IntWidth::W64)),
                        field("byte", 1, u(IntWidth::W8)),
                    ],
                ),
                variant("Word", 1, vec![field("word", 0, u(IntWidth::W32))]),
            ],
        )]);
        let value = decompose(&defs, &Ty::Nominal(DefId(1)));
        // Payload semantic data is 9 bytes at most, plus one-byte tag: three
        // SIA words after coalescing, not four words from the 16-byte payload
        // storage size plus another tag.
        assert_eq!(value.passing, AbiPassing::Direct);
        assert_eq!(value.pieces.len(), 3);
    }

    #[test]
    fn compatible_pointer_sum_keeps_pointer_piece_class() {
        let defs = BTreeMap::from([tagged_def(
            1,
            vec![
                variant("A", 0, vec![field("value", 0, reference())]),
                variant("B", 1, vec![field("value", 0, reference())]),
            ],
        )]);
        let value = decompose(&defs, &Ty::Nominal(DefId(1)));
        assert_eq!(value.pieces.len(), 2);
        assert_eq!(value.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(value.pieces[1].kind, AbiPieceKind::Integer);
    }
}
