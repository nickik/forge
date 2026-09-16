use std::collections::{btree_map::Entry, BTreeMap};

use cranelift_codegen::binemit::Reloc;

use crate::BackendError;

/// One SIA32 object-boundary relocation.
///
/// M8 intentionally models only the relocation contract that exists today:
/// one aligned little-endian 32-bit absolute address word (`Reloc::Abs4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sia32Relocation {
    offset: u32,
    kind: Reloc,
    symbol: String,
    addend: i32,
}

impl Sia32Relocation {
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    pub const fn kind(&self) -> Reloc {
        self.kind
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub const fn addend(&self) -> i32 {
        self.addend
    }
}

/// Minimal SIA32 relocatable object used by Forge before a standard SIA ELF
/// machine identity exists.
///
/// The byte payload may represent executable text or data. Symbols are offsets
/// within that payload; final object placement is supplied by the linker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sia32Object {
    bytes: Vec<u8>,
    symbols: BTreeMap<String, u32>,
    relocations: Vec<Sia32Relocation>,
}

impl Sia32Object {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            symbols: BTreeMap::new(),
            relocations: Vec::new(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn symbols(&self) -> &BTreeMap<String, u32> {
        &self.symbols
    }

    pub fn relocations(&self) -> &[Sia32Relocation] {
        &self.relocations
    }

    /// Define a symbol at an offset in this object's payload.
    pub fn define_symbol(
        &mut self,
        name: impl Into<String>,
        offset: u32,
    ) -> Result<(), BackendError> {
        let name = name.into();
        if u64::from(offset) > self.bytes.len() as u64 {
            return Err(sia32_object_error(format!(
                "SIA32 symbol {name:?} offset {offset} exceeds object size {}",
                self.bytes.len()
            )));
        }
        match self.symbols.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(offset);
            }
            Entry::Occupied(entry) => {
                return Err(sia32_object_error(format!(
                    "duplicate SIA32 object symbol {:?}",
                    entry.key()
                )));
            }
        }
        Ok(())
    }

    /// Record the canonical SIA32 ABS32 relocation at an aligned 4-byte word.
    pub fn add_abs32_relocation(
        &mut self,
        offset: u32,
        symbol: impl Into<String>,
        addend: i32,
    ) -> Result<(), BackendError> {
        self.add_relocation(offset, Reloc::Abs4, symbol, addend)
    }

    /// Record a relocation while validating the object-boundary field.
    ///
    /// This generic entry point exists so unsupported Cranelift relocation
    /// kinds can be rejected explicitly rather than silently reinterpreted.
    pub fn add_relocation(
        &mut self,
        offset: u32,
        kind: Reloc,
        symbol: impl Into<String>,
        addend: i32,
    ) -> Result<(), BackendError> {
        if kind != Reloc::Abs4 {
            return Err(sia32_object_error(format!(
                "unsupported SIA32 object relocation kind {kind:?}"
            )));
        }
        if !offset.is_multiple_of(4) {
            return Err(sia32_object_error(format!(
                "SIA32 ABS32 relocation offset {offset} is not 4-byte aligned"
            )));
        }
        let end = u64::from(offset) + 4;
        if end > self.bytes.len() as u64 {
            return Err(sia32_object_error(format!(
                "SIA32 ABS32 relocation at {offset} exceeds object size {}",
                self.bytes.len()
            )));
        }
        self.relocations.push(Sia32Relocation {
            offset,
            kind,
            symbol: symbol.into(),
            addend,
        });
        Ok(())
    }
}

/// Link SIA32 object fragments at explicit 32-bit base addresses.
///
/// The function implements the M8 `S + A` contract only. It does not choose a
/// container format or perform branch relaxation; local PC-relative branches
/// and LDPC literal references must already have been resolved by Cranelift.
pub fn link_sia32_objects(
    objects: &[Sia32Object],
    bases: &[u32],
) -> Result<Vec<Vec<u8>>, BackendError> {
    if objects.len() != bases.len() {
        return Err(sia32_object_error(format!(
            "SIA32 linker received {} objects but {} base addresses",
            objects.len(),
            bases.len()
        )));
    }

    let mut definitions = BTreeMap::<String, u32>::new();
    for (object, base) in objects.iter().zip(bases.iter().copied()) {
        for (name, offset) in object.symbols() {
            let address = base.checked_add(*offset).ok_or_else(|| {
                sia32_object_error(format!(
                    "SIA32 symbol {name:?} address overflows 32-bit address space"
                ))
            })?;
            match definitions.entry(name.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(address);
                }
                Entry::Occupied(entry) => {
                    return Err(sia32_object_error(format!(
                        "duplicate linked SIA32 symbol {:?}",
                        entry.key()
                    )));
                }
            }
        }
    }

    let mut linked = objects
        .iter()
        .map(|object| object.bytes.clone())
        .collect::<Vec<_>>();

    for (object, bytes) in objects.iter().zip(linked.iter_mut()) {
        for relocation in object.relocations() {
            if relocation.kind() != Reloc::Abs4 {
                return Err(sia32_object_error(format!(
                    "unsupported SIA32 object relocation kind {:?}",
                    relocation.kind()
                )));
            }
            let symbol_address = definitions.get(relocation.symbol()).ok_or_else(|| {
                sia32_object_error(format!("unresolved SIA32 symbol {:?}", relocation.symbol()))
            })?;
            let value = i128::from(*symbol_address) + i128::from(relocation.addend());
            let value = u32::try_from(value).map_err(|_| {
                sia32_object_error(format!(
                    "SIA32 ABS32 relocation for {:?} overflows 32-bit address space: S={:#x}, A={}",
                    relocation.symbol(),
                    symbol_address,
                    relocation.addend()
                ))
            })?;
            let offset = usize::try_from(relocation.offset())
                .map_err(|_| sia32_object_error("SIA32 relocation offset does not fit usize"))?;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    Ok(linked)
}

fn sia32_object_error(message: impl Into<String>) -> BackendError {
    BackendError::Cranelift {
        message: message.into(),
    }
}
