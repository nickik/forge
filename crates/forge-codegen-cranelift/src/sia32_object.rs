use std::collections::{btree_map::Entry, BTreeMap};

use cranelift_codegen::binemit::Reloc;

use crate::BackendError;

const SIAO_MAGIC: &[u8; 8] = b"SIAO32\0\x01";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sia32Section {
    Text,
    Rodata,
    Data,
}

impl Sia32Section {
    const fn tag(self) -> u8 {
        match self {
            Self::Text => 1,
            Self::Rodata => 2,
            Self::Data => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, BackendError> {
        match tag {
            1 => Ok(Self::Text),
            2 => Ok(Self::Rodata),
            3 => Ok(Self::Data),
            _ => Err(sia32_object_error(format!("unknown SIA32 object section tag {tag}"))),
        }
    }
}

/// One SIA32 object-boundary relocation. M8 deliberately exposes one kind:
/// an aligned little-endian 32-bit absolute address word (`Reloc::Abs4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sia32Relocation {
    section: Sia32Section,
    offset: u32,
    kind: Reloc,
    symbol: String,
    addend: i32,
}

impl Sia32Relocation {
    pub const fn section(&self) -> Sia32Section { self.section }
    pub const fn offset(&self) -> u32 { self.offset }
    pub const fn kind(&self) -> Reloc { self.kind }
    pub fn symbol(&self) -> &str { &self.symbol }
    pub const fn addend(&self) -> i32 { self.addend }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sia32Symbol {
    section: Sia32Section,
    offset: u32,
}

impl Sia32Symbol {
    pub const fn section(&self) -> Sia32Section { self.section }
    pub const fn offset(&self) -> u32 { self.offset }
}

/// Explicit M8 relocatable object format used until SIA has an assigned ELF
/// machine identity. `to_bytes` is the real object writer and `from_bytes` is
/// its strict parser; the format carries .text/.rodata/.data, symbols and
/// ABS32 relocations without borrowing another architecture's ELF identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sia32Object {
    text: Vec<u8>,
    rodata: Vec<u8>,
    data: Vec<u8>,
    symbols: BTreeMap<String, Sia32Symbol>,
    relocations: Vec<Sia32Relocation>,
}

impl Sia32Object {
    /// Compatibility constructor: the supplied payload is `.text`.
    pub fn new(text: Vec<u8>) -> Self {
        Self { text, rodata: Vec::new(), data: Vec::new(), symbols: BTreeMap::new(), relocations: Vec::new() }
    }

    pub fn with_sections(text: Vec<u8>, rodata: Vec<u8>, data: Vec<u8>) -> Self {
        Self { text, rodata, data, symbols: BTreeMap::new(), relocations: Vec::new() }
    }

    pub fn bytes(&self) -> &[u8] { &self.text }
    pub fn section(&self, section: Sia32Section) -> &[u8] {
        match section { Sia32Section::Text => &self.text, Sia32Section::Rodata => &self.rodata, Sia32Section::Data => &self.data }
    }
    fn section_mut(&mut self, section: Sia32Section) -> &mut Vec<u8> {
        match section { Sia32Section::Text => &mut self.text, Sia32Section::Rodata => &mut self.rodata, Sia32Section::Data => &mut self.data }
    }
    pub fn symbols(&self) -> &BTreeMap<String, Sia32Symbol> { &self.symbols }
    pub fn relocations(&self) -> &[Sia32Relocation] { &self.relocations }

    pub fn define_symbol(&mut self, name: impl Into<String>, offset: u32) -> Result<(), BackendError> {
        self.define_section_symbol(name, Sia32Section::Text, offset)
    }

    pub fn define_section_symbol(&mut self, name: impl Into<String>, section: Sia32Section, offset: u32) -> Result<(), BackendError> {
        let name = name.into();
        if u64::from(offset) > self.section(section).len() as u64 {
            return Err(sia32_object_error(format!("SIA32 symbol {name:?} offset {offset} exceeds {section:?} size {}", self.section(section).len())));
        }
        match self.symbols.entry(name) {
            Entry::Vacant(entry) => { entry.insert(Sia32Symbol { section, offset }); }
            Entry::Occupied(entry) => return Err(sia32_object_error(format!("duplicate SIA32 object symbol {:?}", entry.key()))),
        }
        Ok(())
    }

    pub fn add_abs32_relocation(&mut self, offset: u32, symbol: impl Into<String>, addend: i32) -> Result<(), BackendError> {
        self.add_section_abs32_relocation(Sia32Section::Text, offset, symbol, addend)
    }

    pub fn add_section_abs32_relocation(&mut self, section: Sia32Section, offset: u32, symbol: impl Into<String>, addend: i32) -> Result<(), BackendError> {
        self.add_section_relocation(section, offset, Reloc::Abs4, symbol, addend)
    }

    pub fn add_relocation(&mut self, offset: u32, kind: Reloc, symbol: impl Into<String>, addend: i32) -> Result<(), BackendError> {
        self.add_section_relocation(Sia32Section::Text, offset, kind, symbol, addend)
    }

    pub fn add_section_relocation(&mut self, section: Sia32Section, offset: u32, kind: Reloc, symbol: impl Into<String>, addend: i32) -> Result<(), BackendError> {
        if kind != Reloc::Abs4 { return Err(sia32_object_error(format!("unsupported SIA32 object relocation kind {kind:?}"))); }
        if !offset.is_multiple_of(4) { return Err(sia32_object_error(format!("SIA32 ABS32 relocation offset {offset} is not 4-byte aligned"))); }
        if u64::from(offset) + 4 > self.section(section).len() as u64 {
            return Err(sia32_object_error(format!("SIA32 ABS32 relocation at {offset} exceeds {section:?} size {}", self.section(section).len())));
        }
        self.relocations.push(Sia32Relocation { section, offset, kind, symbol: symbol.into(), addend });
        Ok(())
    }

    /// Serialize the explicit SIAO32 object. All integer fields are little-endian.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BackendError> {
        let mut out = Vec::new();
        out.extend_from_slice(SIAO_MAGIC);
        put_u32(&mut out, checked_u32(self.text.len(), "text size")?);
        put_u32(&mut out, checked_u32(self.rodata.len(), "rodata size")?);
        put_u32(&mut out, checked_u32(self.data.len(), "data size")?);
        put_u32(&mut out, checked_u32(self.symbols.len(), "symbol count")?);
        put_u32(&mut out, checked_u32(self.relocations.len(), "relocation count")?);
        out.extend_from_slice(&self.text);
        out.extend_from_slice(&self.rodata);
        out.extend_from_slice(&self.data);
        for (name, symbol) in &self.symbols {
            out.push(symbol.section.tag());
            put_u32(&mut out, symbol.offset);
            put_string(&mut out, name)?;
        }
        for relocation in &self.relocations {
            out.push(relocation.section.tag());
            out.push(1); // ABS32
            put_u32(&mut out, relocation.offset);
            out.extend_from_slice(&relocation.addend.to_le_bytes());
            put_string(&mut out, &relocation.symbol)?;
        }
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BackendError> {
        let mut input = Input::new(bytes);
        if input.take(8)? != SIAO_MAGIC { return Err(sia32_object_error("invalid SIAO32 object magic/version")); }
        let text_len = input.u32()? as usize;
        let rodata_len = input.u32()? as usize;
        let data_len = input.u32()? as usize;
        let symbol_count = input.u32()?;
        let relocation_count = input.u32()?;
        let text = input.take(text_len)?.to_vec();
        let rodata = input.take(rodata_len)?.to_vec();
        let data = input.take(data_len)?.to_vec();
        let mut object = Self::with_sections(text, rodata, data);
        for _ in 0..symbol_count {
            let section = Sia32Section::from_tag(input.u8()?)?;
            let offset = input.u32()?;
            let name = input.string()?;
            object.define_section_symbol(name, section, offset)?;
        }
        for _ in 0..relocation_count {
            let section = Sia32Section::from_tag(input.u8()?)?;
            if input.u8()? != 1 { return Err(sia32_object_error("unsupported SIAO32 relocation tag")); }
            let offset = input.u32()?;
            let addend = input.i32()?;
            let symbol = input.string()?;
            object.add_section_abs32_relocation(section, offset, symbol, addend)?;
        }
        if !input.is_empty() { return Err(sia32_object_error("trailing bytes after SIAO32 object")); }
        Ok(object)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sia32SectionBases { pub text: u32, pub rodata: u32, pub data: u32 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSia32Object { pub text: Vec<u8>, pub rodata: Vec<u8>, pub data: Vec<u8> }

/// Link parsed/constructed SIA32 objects at explicit per-section addresses.
pub fn link_sia32_sectioned_objects(objects: &[Sia32Object], bases: &[Sia32SectionBases]) -> Result<Vec<LinkedSia32Object>, BackendError> {
    if objects.len() != bases.len() { return Err(sia32_object_error(format!("SIA32 linker received {} objects but {} base layouts", objects.len(), bases.len()))); }
    let mut definitions = BTreeMap::<String, u32>::new();
    for (object, base) in objects.iter().zip(bases) {
        for (name, symbol) in object.symbols() {
            let section_base = match symbol.section { Sia32Section::Text => base.text, Sia32Section::Rodata => base.rodata, Sia32Section::Data => base.data };
            let address = section_base.checked_add(symbol.offset).ok_or_else(|| sia32_object_error(format!("SIA32 symbol {name:?} address overflows 32-bit address space")))?;
            match definitions.entry(name.clone()) {
                Entry::Vacant(entry) => { entry.insert(address); }
                Entry::Occupied(entry) => return Err(sia32_object_error(format!("duplicate linked SIA32 symbol {:?}", entry.key()))),
            }
        }
    }
    let mut linked = objects.iter().map(|o| LinkedSia32Object { text: o.text.clone(), rodata: o.rodata.clone(), data: o.data.clone() }).collect::<Vec<_>>();
    for (object, output) in objects.iter().zip(&mut linked) {
        for relocation in object.relocations() {
            let symbol_address = definitions.get(relocation.symbol()).ok_or_else(|| sia32_object_error(format!("unresolved SIA32 symbol {:?}", relocation.symbol())))?;
            let value = i128::from(*symbol_address) + i128::from(relocation.addend());
            let value = u32::try_from(value).map_err(|_| sia32_object_error(format!("SIA32 ABS32 relocation for {:?} overflows 32-bit address space: S={:#x}, A={}", relocation.symbol(), symbol_address, relocation.addend())))?;
            let bytes = match relocation.section { Sia32Section::Text => &mut output.text, Sia32Section::Rodata => &mut output.rodata, Sia32Section::Data => &mut output.data };
            let offset = relocation.offset as usize;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(linked)
}

/// Legacy flat-text M8 linker retained for callers that have no data sections.
pub fn link_sia32_objects(objects: &[Sia32Object], bases: &[u32]) -> Result<Vec<Vec<u8>>, BackendError> {
    let layouts = bases.iter().copied().map(|base| Sia32SectionBases { text: base, rodata: base, data: base }).collect::<Vec<_>>();
    Ok(link_sia32_sectioned_objects(objects, &layouts)?.into_iter().map(|o| o.text).collect())
}

fn checked_u32(value: usize, what: &str) -> Result<u32, BackendError> { u32::try_from(value).map_err(|_| sia32_object_error(format!("SIAO32 {what} exceeds u32"))) }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_string(out: &mut Vec<u8>, value: &str) -> Result<(), BackendError> { put_u32(out, checked_u32(value.len(), "string length")?); out.extend_from_slice(value.as_bytes()); Ok(()) }

struct Input<'a> { bytes: &'a [u8], pos: usize }
impl<'a> Input<'a> {
    fn new(bytes: &'a [u8]) -> Self { Self { bytes, pos: 0 } }
    fn take(&mut self, count: usize) -> Result<&'a [u8], BackendError> { let end = self.pos.checked_add(count).ok_or_else(|| sia32_object_error("SIAO32 parse offset overflow"))?; let result = self.bytes.get(self.pos..end).ok_or_else(|| sia32_object_error("truncated SIAO32 object"))?; self.pos = end; Ok(result) }
    fn u8(&mut self) -> Result<u8, BackendError> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<u32, BackendError> { Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes"))) }
    fn i32(&mut self) -> Result<i32, BackendError> { Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("four bytes"))) }
    fn string(&mut self) -> Result<String, BackendError> { let len = self.u32()? as usize; String::from_utf8(self.take(len)?.to_vec()).map_err(|_| sia32_object_error("non-UTF-8 SIAO32 symbol")) }
    fn is_empty(&self) -> bool { self.pos == self.bytes.len() }
}

fn sia32_object_error(message: impl Into<String>) -> BackendError { BackendError::Cranelift { message: message.into() } }
