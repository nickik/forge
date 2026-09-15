use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{ExternalName, Function, Signature};
use cranelift_codegen::{Context, RelocTarget};
use forge_fir::DefId;

use crate::{BackendError, CraneliftBackend, CraneliftTarget, PreparedModule};

/// Linkage policy at the Forge object boundary.
///
/// C10a keeps functions local unless the caller explicitly marks a FIR
/// definition for export. Source-language export semantics remain a frontend
/// concern and are not inferred from function names here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectLinkage {
    Local,
    Export,
}

/// One deterministic function symbol planned for object emission.
#[derive(Clone, Debug)]
pub struct ObjectSymbol {
    owner: DefId,
    name: String,
    linkage: ObjectLinkage,
    signature: Signature,
}

impl ObjectSymbol {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn linkage(&self) -> ObjectLinkage {
        self.linkage
    }

    /// The exact CLIF signature produced by the C9 Forge ABI lowering.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// Deterministic symbol/linkage plan consumed by native object emission.
#[derive(Clone, Debug)]
pub struct ObjectModulePlan {
    target: CraneliftTarget,
    symbols: BTreeMap<DefId, ObjectSymbol>,
}

impl ObjectModulePlan {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn symbols(&self) -> &BTreeMap<DefId, ObjectSymbol> {
        &self.symbols
    }

    pub fn symbol(&self, owner: DefId) -> Option<&ObjectSymbol> {
        self.symbols.get(&owner)
    }
}

/// A deterministic ELF64 relocatable object emitted from one prepared Forge
/// module. C10 currently supports the two Linux targets represented by
/// [`CraneliftTarget`].
#[derive(Clone, Debug)]
pub struct NativeObject {
    target: CraneliftTarget,
    plan: ObjectModulePlan,
    bytes: Vec<u8>,
}

impl NativeObject {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn plan(&self) -> &ObjectModulePlan {
        &self.plan
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl CraneliftBackend {
    /// Plan a module with all FIR functions kept local.
    pub fn plan_object_module(
        &self,
        prepared: &PreparedModule,
    ) -> Result<ObjectModulePlan, BackendError> {
        self.plan_object_module_with_exports(prepared, std::iter::empty())
    }

    /// Plan deterministic object symbols while explicitly promoting selected
    /// FIR definitions to exported linkage.
    pub fn plan_object_module_with_exports<I>(
        &self,
        prepared: &PreparedModule,
        exports: I,
    ) -> Result<ObjectModulePlan, BackendError>
    where
        I: IntoIterator<Item = DefId>,
    {
        if prepared.target() != self.target() {
            return Err(shape(format!(
                "prepared module target {:?} does not match object-plan target {:?}",
                prepared.target(),
                self.target()
            )));
        }

        let exports: BTreeSet<DefId> = exports.into_iter().collect();
        for owner in &exports {
            if !prepared.functions().contains_key(owner) {
                return Err(shape(format!(
                    "cannot export missing FIR function {owner:?}"
                )));
            }
        }

        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::new();
        for (owner, function) in prepared.functions() {
            let name = forge_function_symbol(*owner);
            if !names.insert(name.clone()) {
                return Err(shape(format!(
                    "duplicate Forge object symbol generated for {owner:?}: {name}"
                )));
            }
            let linkage = if exports.contains(owner) {
                ObjectLinkage::Export
            } else {
                ObjectLinkage::Local
            };
            symbols.insert(
                *owner,
                ObjectSymbol {
                    owner: *owner,
                    name,
                    linkage,
                    signature: function.signature.clone(),
                },
            );
        }

        Ok(ObjectModulePlan {
            target: self.target(),
            symbols,
        })
    }

    /// Emit one deterministic ELF64 relocatable object using an already
    /// validated C10 symbol plan. Direct-call and function-address relocations
    /// remain symbolic for the system linker to resolve.
    pub fn emit_object(
        &self,
        prepared: &PreparedModule,
        plan: &ObjectModulePlan,
    ) -> Result<NativeObject, BackendError> {
        validate_object_plan(self.target(), prepared, plan)?;
        let isa = self.target().isa()?;
        let mut text = Vec::new();
        let mut functions = BTreeMap::new();
        let mut relocs = Vec::new();
        let mut labels = BTreeSet::new();
        let mut section_alignment = 1u64;

        for (owner, function) in prepared.functions() {
            let mut context = Context::for_function(function.clone());
            let mut control = ControlPlane::default();
            let compiled = context.compile(&*isa, &mut control).map_err(|error| {
                object_error(format!(
                    "object compilation failed for {owner:?}: {error:?}"
                ))
            })?;

            let alignment = u64::from(compiled.buffer.min_alignment.max(1));
            section_alignment = section_alignment.max(alignment);
            align_vec(&mut text, alignment)?;
            let start = text.len() as u64;
            let code = compiled.code_buffer();
            if code.is_empty() {
                return Err(object_error(format!(
                    "object compilation produced no bytes for {owner:?}"
                )));
            }
            text.extend_from_slice(code);
            functions.insert(
                *owner,
                EmittedFunction {
                    start,
                    size: code.len() as u64,
                },
            );

            for reloc in compiled.buffer.relocs() {
                let target = relocation_target(*owner, function, start, code.len(), reloc)?;
                if let RelocationTarget::Label { owner, offset } = target {
                    labels.insert((owner, offset));
                }
                relocs.push(PendingRelocation {
                    offset: start + u64::from(reloc.offset),
                    kind: reloc.kind,
                    target,
                    addend: reloc.addend,
                });
            }
        }

        let bytes = emit_elf64(
            self.target(),
            plan,
            &text,
            section_alignment,
            &functions,
            &labels,
            &relocs,
        )?;
        Ok(NativeObject {
            target: self.target(),
            plan: plan.clone(),
            bytes,
        })
    }

    /// Plan and emit an object in one operation.
    pub fn emit_object_with_exports<I>(
        &self,
        prepared: &PreparedModule,
        exports: I,
    ) -> Result<NativeObject, BackendError>
    where
        I: IntoIterator<Item = DefId>,
    {
        let plan = self.plan_object_module_with_exports(prepared, exports)?;
        self.emit_object(prepared, &plan)
    }
}

fn validate_object_plan(
    target: CraneliftTarget,
    prepared: &PreparedModule,
    plan: &ObjectModulePlan,
) -> Result<(), BackendError> {
    if prepared.target() != target || plan.target() != target {
        return Err(shape(format!(
            "object emission target mismatch: backend={target:?}, prepared={:?}, plan={:?}",
            prepared.target(),
            plan.target()
        )));
    }
    if prepared.functions().len() != plan.symbols().len() {
        return Err(shape(format!(
            "object plan has {} symbols for {} prepared functions",
            plan.symbols().len(),
            prepared.functions().len()
        )));
    }
    for (owner, function) in prepared.functions() {
        let symbol = plan
            .symbol(*owner)
            .ok_or_else(|| shape(format!("object plan is missing function {owner:?}")))?;
        if symbol.signature() != &function.signature {
            return Err(shape(format!(
                "object plan has stale C9 ABI signature for {owner:?}"
            )));
        }
    }
    for owner in plan.symbols().keys() {
        if !prepared.functions().contains_key(owner) {
            return Err(shape(format!(
                "object plan contains stale function symbol {owner:?}"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct EmittedFunction {
    start: u64,
    size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RelocationTarget {
    Function(DefId),
    Label { owner: DefId, offset: u32 },
}

#[derive(Clone, Copy, Debug)]
struct PendingRelocation {
    offset: u64,
    kind: Reloc,
    target: RelocationTarget,
    addend: i64,
}

fn relocation_target(
    owner: DefId,
    function: &Function,
    _function_start: u64,
    function_size: usize,
    reloc: &cranelift_codegen::MachReloc,
) -> Result<RelocationTarget, BackendError> {
    match &reloc.target {
        RelocTarget::ExternalName(ExternalName::User(reference)) => {
            let name = &function.params.user_named_funcs()[*reference];
            if name.namespace != 0 {
                return Err(object_error(format!(
                    "unsupported Forge object relocation namespace {} in {owner:?}",
                    name.namespace
                )));
            }
            Ok(RelocationTarget::Function(DefId(name.index)))
        }
        RelocTarget::Label(label) => {
            let offset = label.as_offset();
            if offset as usize > function_size {
                return Err(object_error(format!(
                    "relocation label offset {offset} escapes function {owner:?} ({function_size} bytes)"
                )));
            }
            Ok(RelocationTarget::Label { owner, offset })
        }
        target => Err(object_error(format!(
            "unsupported Forge object relocation target in {owner:?}: {target:?}"
        ))),
    }
}

fn emit_elf64(
    target: CraneliftTarget,
    plan: &ObjectModulePlan,
    text: &[u8],
    text_alignment: u64,
    functions: &BTreeMap<DefId, EmittedFunction>,
    labels: &BTreeSet<(DefId, u32)>,
    relocs: &[PendingRelocation],
) -> Result<Vec<u8>, BackendError> {
    const TEXT_SECTION: u16 = 1;
    const RELA_TEXT_SECTION: u32 = 2;
    const SYMTAB_SECTION: u32 = 3;
    const STRTAB_SECTION: u32 = 4;
    const SHSTRTAB_SECTION: u16 = 5;
    const SECTION_COUNT: u16 = 7;

    let mut strtab = vec![0u8];
    let mut symbols = vec![ElfSymbol::null()];
    let mut function_symbols = BTreeMap::new();
    let mut label_symbols = BTreeMap::new();

    for (owner, symbol) in plan
        .symbols()
        .iter()
        .filter(|(_, symbol)| symbol.linkage() == ObjectLinkage::Local)
    {
        let emitted = functions.get(owner).ok_or_else(|| {
            object_error(format!(
                "no emitted machine code for local function {owner:?}"
            ))
        })?;
        let name = add_string(&mut strtab, symbol.name())?;
        let index = symbols.len() as u32;
        symbols.push(ElfSymbol::function(
            name,
            false,
            TEXT_SECTION,
            emitted.start,
            emitted.size,
        ));
        function_symbols.insert(*owner, index);
    }

    for &(owner, offset) in labels {
        let emitted = functions.get(&owner).ok_or_else(|| {
            object_error(format!(
                "relocation label refers to missing function {owner:?}"
            ))
        })?;
        if u64::from(offset) > emitted.size {
            return Err(object_error(format!(
                "relocation label {owner:?}+{offset} exceeds emitted function size {}",
                emitted.size
            )));
        }
        let index = symbols.len() as u32;
        symbols.push(ElfSymbol::label(
            TEXT_SECTION,
            emitted.start + u64::from(offset),
        ));
        label_symbols.insert((owner, offset), index);
    }

    let first_global = symbols.len() as u32;
    for (owner, symbol) in plan
        .symbols()
        .iter()
        .filter(|(_, symbol)| symbol.linkage() == ObjectLinkage::Export)
    {
        let emitted = functions.get(owner).ok_or_else(|| {
            object_error(format!(
                "no emitted machine code for exported function {owner:?}"
            ))
        })?;
        let name = add_string(&mut strtab, symbol.name())?;
        let index = symbols.len() as u32;
        symbols.push(ElfSymbol::function(
            name,
            true,
            TEXT_SECTION,
            emitted.start,
            emitted.size,
        ));
        function_symbols.insert(*owner, index);
    }

    if function_symbols.len() != functions.len() {
        return Err(object_error(
            "not every emitted function received an ELF symbol",
        ));
    }

    let mut rela_text = Vec::with_capacity(relocs.len() * 24);
    for reloc in relocs {
        let symbol = match reloc.target {
            RelocationTarget::Function(owner) => {
                *function_symbols.get(&owner).ok_or_else(|| {
                    object_error(format!(
                        "unresolved Forge relocation target {owner:?}; target is not in object plan"
                    ))
                })?
            }
            RelocationTarget::Label { owner, offset } => {
                *label_symbols.get(&(owner, offset)).ok_or_else(|| {
                    object_error(format!(
                        "unresolved internal relocation label {owner:?}+{offset}"
                    ))
                })?
            }
        };
        let relocation_type = elf_relocation_type(target, reloc.kind)?;
        push_u64(&mut rela_text, reloc.offset);
        push_u64(
            &mut rela_text,
            (u64::from(symbol) << 32) | u64::from(relocation_type),
        );
        push_i64(&mut rela_text, reloc.addend);
    }

    let mut symtab = Vec::with_capacity(symbols.len() * 24);
    for symbol in &symbols {
        symbol.write_to(&mut symtab);
    }

    let mut shstrtab = vec![0u8];
    let text_name = add_string(&mut shstrtab, ".text")?;
    let rela_text_name = add_string(&mut shstrtab, ".rela.text")?;
    let symtab_name = add_string(&mut shstrtab, ".symtab")?;
    let strtab_name = add_string(&mut shstrtab, ".strtab")?;
    let shstrtab_name = add_string(&mut shstrtab, ".shstrtab")?;
    let stack_name = add_string(&mut shstrtab, ".note.GNU-stack")?;

    let mut output = vec![0u8; 64];
    let text_offset = append_section(&mut output, text, text_alignment.max(1))?;
    let rela_text_offset = append_section(&mut output, &rela_text, 8)?;
    let symtab_offset = append_section(&mut output, &symtab, 8)?;
    let strtab_offset = append_section(&mut output, &strtab, 1)?;
    let shstrtab_offset = append_section(&mut output, &shstrtab, 1)?;
    let stack_offset = output.len() as u64;
    align_vec(&mut output, 8)?;
    let section_headers_offset = output.len() as u64;

    SectionHeader::null().write_to(&mut output);
    SectionHeader {
        name: text_name,
        kind: 1,
        flags: 0x6,
        offset: text_offset,
        size: text.len() as u64,
        link: 0,
        info: 0,
        alignment: text_alignment.max(1),
        entry_size: 0,
    }
    .write_to(&mut output);
    SectionHeader {
        name: rela_text_name,
        kind: 4,
        flags: 0,
        offset: rela_text_offset,
        size: rela_text.len() as u64,
        link: SYMTAB_SECTION,
        info: u32::from(TEXT_SECTION),
        alignment: 8,
        entry_size: 24,
    }
    .write_to(&mut output);
    SectionHeader {
        name: symtab_name,
        kind: 2,
        flags: 0,
        offset: symtab_offset,
        size: symtab.len() as u64,
        link: STRTAB_SECTION,
        info: first_global,
        alignment: 8,
        entry_size: 24,
    }
    .write_to(&mut output);
    SectionHeader {
        name: strtab_name,
        kind: 3,
        flags: 0,
        offset: strtab_offset,
        size: strtab.len() as u64,
        link: 0,
        info: 0,
        alignment: 1,
        entry_size: 0,
    }
    .write_to(&mut output);
    SectionHeader {
        name: shstrtab_name,
        kind: 3,
        flags: 0,
        offset: shstrtab_offset,
        size: shstrtab.len() as u64,
        link: 0,
        info: 0,
        alignment: 1,
        entry_size: 0,
    }
    .write_to(&mut output);
    SectionHeader {
        name: stack_name,
        kind: 1,
        flags: 0,
        offset: stack_offset,
        size: 0,
        link: 0,
        info: 0,
        alignment: 1,
        entry_size: 0,
    }
    .write_to(&mut output);

    write_elf_header(
        &mut output[..64],
        target,
        section_headers_offset,
        SECTION_COUNT,
        SHSTRTAB_SECTION,
    );
    Ok(output)
}

fn elf_relocation_type(target: CraneliftTarget, kind: Reloc) -> Result<u32, BackendError> {
    match (target, kind) {
        (CraneliftTarget::Aarch64, Reloc::Abs4) => Ok(258),
        (CraneliftTarget::Aarch64, Reloc::Abs8) => Ok(257),
        (CraneliftTarget::Aarch64, Reloc::Arm64Call) => Ok(283),
        (CraneliftTarget::Aarch64, Reloc::Aarch64AdrPrelPgHi21) => Ok(275),
        (CraneliftTarget::Aarch64, Reloc::Aarch64AddAbsLo12Nc) => Ok(277),
        (CraneliftTarget::Aarch64, Reloc::Aarch64AdrGotPage21) => Ok(311),
        (CraneliftTarget::Aarch64, Reloc::Aarch64Ld64GotLo12Nc) => Ok(312),
        (CraneliftTarget::Riscv64, Reloc::Abs4) => Ok(1),
        (CraneliftTarget::Riscv64, Reloc::Abs8) => Ok(2),
        (CraneliftTarget::Riscv64, Reloc::RiscvCallPlt) => Ok(19),
        (CraneliftTarget::Riscv64, Reloc::RiscvGotHi20) => Ok(20),
        (CraneliftTarget::Riscv64, Reloc::RiscvPCRelHi20) => Ok(23),
        (CraneliftTarget::Riscv64, Reloc::RiscvPCRelLo12I) => Ok(24),
        (target, kind) => Err(object_error(format!(
            "unsupported {target:?} object relocation kind {kind:?}"
        ))),
    }
}

#[derive(Clone, Copy)]
struct ElfSymbol {
    name: u32,
    info: u8,
    other: u8,
    section: u16,
    value: u64,
    size: u64,
}

impl ElfSymbol {
    const fn null() -> Self {
        Self {
            name: 0,
            info: 0,
            other: 0,
            section: 0,
            value: 0,
            size: 0,
        }
    }

    const fn function(name: u32, global: bool, section: u16, value: u64, size: u64) -> Self {
        let binding = if global { 1 } else { 0 };
        Self {
            name,
            info: (binding << 4) | 2,
            other: 0,
            section,
            value,
            size,
        }
    }

    const fn label(section: u16, value: u64) -> Self {
        Self {
            name: 0,
            info: 0,
            other: 0,
            section,
            value,
            size: 0,
        }
    }

    fn write_to(self, output: &mut Vec<u8>) {
        push_u32(output, self.name);
        output.push(self.info);
        output.push(self.other);
        push_u16(output, self.section);
        push_u64(output, self.value);
        push_u64(output, self.size);
    }
}

#[derive(Clone, Copy)]
struct SectionHeader {
    name: u32,
    kind: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    alignment: u64,
    entry_size: u64,
}

impl SectionHeader {
    const fn null() -> Self {
        Self {
            name: 0,
            kind: 0,
            flags: 0,
            offset: 0,
            size: 0,
            link: 0,
            info: 0,
            alignment: 0,
            entry_size: 0,
        }
    }

    fn write_to(self, output: &mut Vec<u8>) {
        push_u32(output, self.name);
        push_u32(output, self.kind);
        push_u64(output, self.flags);
        push_u64(output, 0);
        push_u64(output, self.offset);
        push_u64(output, self.size);
        push_u32(output, self.link);
        push_u32(output, self.info);
        push_u64(output, self.alignment);
        push_u64(output, self.entry_size);
    }
}

fn write_elf_header(
    header: &mut [u8],
    target: CraneliftTarget,
    section_headers_offset: u64,
    section_count: u16,
    section_names: u16,
) {
    header[0..4].copy_from_slice(b"\x7fELF");
    header[4] = 2;
    header[5] = 1;
    header[6] = 1;
    write_u16_at(header, 16, 1);
    write_u16_at(
        header,
        18,
        match target {
            CraneliftTarget::Aarch64 => 183,
            CraneliftTarget::Riscv64 => 243,
        },
    );
    write_u32_at(header, 20, 1);
    write_u64_at(header, 40, section_headers_offset);
    write_u32_at(
        header,
        48,
        match target {
            CraneliftTarget::Aarch64 => 0,
            CraneliftTarget::Riscv64 => 0x5,
        },
    );
    write_u16_at(header, 52, 64);
    write_u16_at(header, 58, 64);
    write_u16_at(header, 60, section_count);
    write_u16_at(header, 62, section_names);
}

fn append_section(output: &mut Vec<u8>, data: &[u8], alignment: u64) -> Result<u64, BackendError> {
    align_vec(output, alignment)?;
    let offset = output.len() as u64;
    output.extend_from_slice(data);
    Ok(offset)
}

fn align_vec(output: &mut Vec<u8>, alignment: u64) -> Result<(), BackendError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(object_error(format!(
            "invalid object alignment {alignment}"
        )));
    }
    let len = output.len() as u64;
    let aligned = len
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| object_error("object alignment overflow"))?;
    let padding = usize::try_from(aligned - len)
        .map_err(|_| object_error("object alignment padding does not fit usize"))?;
    output.resize(output.len() + padding, 0);
    Ok(())
}

fn add_string(table: &mut Vec<u8>, value: &str) -> Result<u32, BackendError> {
    if value.as_bytes().contains(&0) {
        return Err(object_error(format!(
            "object symbol/section name contains NUL: {value:?}"
        )));
    }
    let offset = u32::try_from(table.len())
        .map_err(|_| object_error("object string table exceeds ELF32 string offset space"))?;
    table.extend_from_slice(value.as_bytes());
    table.push(0);
    Ok(offset)
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u16_at(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_at(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_at(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn forge_function_symbol(owner: DefId) -> String {
    format!("__forge_fn_{:08x}", owner.0)
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}

fn object_error(message: impl Into<String>) -> BackendError {
    BackendError::Cranelift {
        message: message.into(),
    }
}
