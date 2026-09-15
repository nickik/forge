use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{ExternalName, Function, Signature};
use cranelift_codegen::{Context, RelocTarget};
use forge_fir::{DefId, StaticSymbol};

use crate::{
    BackendError, CraneliftBackend, CraneliftTarget, GlobalObjectSymbol, GlobalStorageClass,
    PreparedModule, PreparedStaticRelocation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectLinkage {
    Local,
    Export,
}

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

    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

#[derive(Clone, Debug)]
pub struct ObjectModulePlan {
    target: CraneliftTarget,
    symbols: BTreeMap<DefId, ObjectSymbol>,
    global_symbols: BTreeMap<DefId, GlobalObjectSymbol>,
    global_init_order: Vec<DefId>,
}

impl ObjectModulePlan {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn symbols(&self) -> &BTreeMap<DefId, ObjectSymbol> {
        &self.symbols
    }

    pub fn function_symbols(&self) -> &BTreeMap<DefId, ObjectSymbol> {
        &self.symbols
    }

    pub fn symbol(&self, owner: DefId) -> Option<&ObjectSymbol> {
        self.symbols.get(&owner)
    }

    pub fn global_symbols(&self) -> &BTreeMap<DefId, GlobalObjectSymbol> {
        &self.global_symbols
    }

    pub fn global_symbol(&self, owner: DefId) -> Option<&GlobalObjectSymbol> {
        self.global_symbols.get(&owner)
    }

    pub fn global_init_order(&self) -> &[DefId] {
        &self.global_init_order
    }
}

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
    pub fn plan_object_module(
        &self,
        prepared: &PreparedModule,
    ) -> Result<ObjectModulePlan, BackendError> {
        self.plan_object_module_with_exports(prepared, std::iter::empty())
    }

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
            if !prepared.functions().contains_key(owner) && !prepared.globals().contains_key(owner)
            {
                return Err(shape(format!(
                    "cannot export missing FIR definition {owner:?}"
                )));
            }
        }
        let function_exports: BTreeSet<DefId> = exports
            .iter()
            .copied()
            .filter(|owner| prepared.functions().contains_key(owner))
            .collect();
        let global_exports: BTreeSet<DefId> = exports
            .iter()
            .copied()
            .filter(|owner| prepared.globals().contains_key(owner))
            .collect();

        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::new();
        for (owner, function) in prepared.functions() {
            let name = forge_function_symbol(*owner);
            if !names.insert(name.clone()) {
                return Err(shape(format!(
                    "duplicate Forge object symbol generated for {owner:?}: {name}"
                )));
            }
            symbols.insert(
                *owner,
                ObjectSymbol {
                    owner: *owner,
                    name,
                    linkage: if function_exports.contains(owner) {
                        ObjectLinkage::Export
                    } else {
                        ObjectLinkage::Local
                    },
                    signature: function.signature.clone(),
                },
            );
        }

        let global_plan =
            self.plan_global_objects_with_exports(prepared.prepared_globals(), global_exports)?;
        for symbol in global_plan.symbols().values() {
            if !names.insert(symbol.name().to_owned()) {
                return Err(shape(format!(
                    "duplicate Forge object symbol generated for global {:?}: {}",
                    symbol.owner(),
                    symbol.name()
                )));
            }
        }

        Ok(ObjectModulePlan {
            target: self.target(),
            symbols,
            global_symbols: global_plan.symbols().clone(),
            global_init_order: prepared.global_init_order().to_vec(),
        })
    }

    pub fn emit_object(
        &self,
        prepared: &PreparedModule,
        plan: &ObjectModulePlan,
    ) -> Result<NativeObject, BackendError> {
        validate_object_plan(self.target(), prepared, plan)?;

        let isa = self.target().isa()?;
        let mut text = Vec::new();
        let mut functions = BTreeMap::new();
        let mut text_relocs = Vec::new();
        let mut labels = BTreeSet::new();
        let mut text_alignment = 1u64;

        for (owner, function) in prepared.functions() {
            let mut context = Context::for_function(function.clone());
            let mut control = ControlPlane::default();
            let compiled = context.compile(&*isa, &mut control).map_err(|error| {
                object_error(format!(
                    "object compilation failed for {owner:?}: {error:?}"
                ))
            })?;

            let alignment = u64::from(compiled.buffer.min_alignment.max(1));
            text_alignment = text_alignment.max(alignment);
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
                let target = relocation_target(*owner, function, code.len(), reloc)?;
                if let TextRelocationTarget::Label { owner, offset } = target {
                    labels.insert((owner, offset));
                }
                text_relocs.push(PendingTextRelocation {
                    offset: start + u64::from(reloc.offset),
                    kind: reloc.kind,
                    target,
                    addend: reloc.addend,
                });
            }
        }

        let globals = emit_global_sections(prepared)?;
        let bytes = emit_elf64(
            self.target(),
            plan,
            &text,
            text_alignment,
            &functions,
            &labels,
            &text_relocs,
            &globals,
        )?;
        Ok(NativeObject {
            target: self.target(),
            plan: plan.clone(),
            bytes,
        })
    }

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
    if prepared.globals().len() != plan.global_symbols().len() {
        return Err(shape(format!(
            "object plan has {} global symbols for {} prepared globals",
            plan.global_symbols().len(),
            prepared.globals().len()
        )));
    }
    if prepared.global_init_order() != plan.global_init_order() {
        return Err(shape("object plan has stale global initializer order"));
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

    for (owner, global) in prepared.globals() {
        let symbol = plan
            .global_symbol(*owner)
            .ok_or_else(|| shape(format!("object plan is missing global {owner:?}")))?;
        if symbol.owner() != *owner
            || symbol.layout() != global.layout()
            || symbol.initialization() != global.initialization()
            || symbol.storage() != global.storage()
            || symbol.static_data() != global.static_data()
        {
            return Err(shape(format!(
                "object plan has stale global metadata for {owner:?}"
            )));
        }
    }
    for owner in plan.global_symbols().keys() {
        if !prepared.globals().contains_key(owner) {
            return Err(shape(format!(
                "object plan contains stale global symbol {owner:?}"
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
enum TextRelocationTarget {
    Function(DefId),
    Label { owner: DefId, offset: u32 },
}

#[derive(Clone, Copy, Debug)]
struct PendingTextRelocation {
    offset: u64,
    kind: Reloc,
    target: TextRelocationTarget,
    addend: i64,
}

fn relocation_target(
    owner: DefId,
    function: &Function,
    function_size: usize,
    reloc: &cranelift_codegen::MachReloc,
) -> Result<TextRelocationTarget, BackendError> {
    match &reloc.target {
        RelocTarget::ExternalName(ExternalName::User(reference)) => {
            let name = &function.params.user_named_funcs()[*reference];
            if name.namespace != 0 {
                return Err(object_error(format!(
                    "unsupported Forge object relocation namespace {} in {owner:?}",
                    name.namespace
                )));
            }
            Ok(TextRelocationTarget::Function(DefId(name.index)))
        }
        RelocTarget::Label(label) => {
            let offset = label.as_offset();
            if offset as usize > function_size {
                return Err(object_error(format!(
                    "relocation label offset {offset} escapes function {owner:?} ({function_size} bytes)"
                )));
            }
            Ok(TextRelocationTarget::Label { owner, offset })
        }
        target => Err(object_error(format!(
            "unsupported Forge object relocation target in {owner:?}: {target:?}"
        ))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataSection {
    ReadOnly,
    Writable,
    Bss,
}

#[derive(Clone, Copy, Debug)]
struct EmittedGlobal {
    section: DataSection,
    offset: u64,
    size: u64,
}

#[derive(Clone, Debug)]
struct GlobalSections {
    rodata: Vec<u8>,
    data: Vec<u8>,
    bss_size: u64,
    rodata_alignment: u64,
    data_alignment: u64,
    bss_alignment: u64,
    globals: BTreeMap<DefId, EmittedGlobal>,
    rodata_relocs: Vec<PendingDataRelocation>,
    data_relocs: Vec<PendingDataRelocation>,
}

#[derive(Clone, Copy, Debug)]
struct PendingDataRelocation {
    offset: u64,
    target: StaticSymbol,
    addend: i64,
    width: u8,
}

fn emit_global_sections(prepared: &PreparedModule) -> Result<GlobalSections, BackendError> {
    let mut sections = GlobalSections {
        rodata: Vec::new(),
        data: Vec::new(),
        bss_size: 0,
        rodata_alignment: 1,
        data_alignment: 1,
        bss_alignment: 1,
        globals: BTreeMap::new(),
        rodata_relocs: Vec::new(),
        data_relocs: Vec::new(),
    };

    for (owner, global) in prepared.globals() {
        let align = global.layout().align.max(1);
        let size = global.layout().size;
        match global.storage() {
            GlobalStorageClass::ReadOnlyData => {
                let static_data = global.static_data().ok_or_else(|| {
                    object_error(format!(
                        "read-only global {owner:?} has no prepared static bytes"
                    ))
                })?;
                validate_static_size(*owner, size, static_data.bytes().len())?;
                sections.rodata_alignment = sections.rodata_alignment.max(align);
                align_vec(&mut sections.rodata, align)?;
                let offset = sections.rodata.len() as u64;
                sections.rodata.extend_from_slice(static_data.bytes());
                append_data_relocations(
                    *owner,
                    offset,
                    static_data.relocations(),
                    &mut sections.rodata_relocs,
                )?;
                sections.globals.insert(
                    *owner,
                    EmittedGlobal {
                        section: DataSection::ReadOnly,
                        offset,
                        size,
                    },
                );
            }
            GlobalStorageClass::WritableData => {
                let static_data = global.static_data().ok_or_else(|| {
                    object_error(format!(
                        "writable global {owner:?} has no prepared static bytes"
                    ))
                })?;
                validate_static_size(*owner, size, static_data.bytes().len())?;
                sections.data_alignment = sections.data_alignment.max(align);
                align_vec(&mut sections.data, align)?;
                let offset = sections.data.len() as u64;
                sections.data.extend_from_slice(static_data.bytes());
                append_data_relocations(
                    *owner,
                    offset,
                    static_data.relocations(),
                    &mut sections.data_relocs,
                )?;
                sections.globals.insert(
                    *owner,
                    EmittedGlobal {
                        section: DataSection::Writable,
                        offset,
                        size,
                    },
                );
            }
            GlobalStorageClass::ZeroFill => {
                if global.static_data().is_some() {
                    return Err(object_error(format!(
                        "zero-fill global {owner:?} unexpectedly carries static bytes"
                    )));
                }
                sections.bss_alignment = sections.bss_alignment.max(align);
                sections.bss_size = align_u64(sections.bss_size, align)?;
                let offset = sections.bss_size;
                sections.bss_size = sections
                    .bss_size
                    .checked_add(size)
                    .ok_or_else(|| object_error(".bss size overflow"))?;
                sections.globals.insert(
                    *owner,
                    EmittedGlobal {
                        section: DataSection::Bss,
                        offset,
                        size,
                    },
                );
            }
        }
    }
    Ok(sections)
}

fn validate_static_size(owner: DefId, expected: u64, actual: usize) -> Result<(), BackendError> {
    if actual as u64 == expected {
        Ok(())
    } else {
        Err(object_error(format!(
            "global {owner:?} has {actual} static bytes for C9 size {expected}"
        )))
    }
}

fn append_data_relocations(
    owner: DefId,
    section_offset: u64,
    relocations: &[PreparedStaticRelocation],
    output: &mut Vec<PendingDataRelocation>,
) -> Result<(), BackendError> {
    for relocation in relocations {
        let offset = section_offset
            .checked_add(relocation.offset())
            .ok_or_else(|| object_error(format!("global {owner:?} relocation offset overflow")))?;
        output.push(PendingDataRelocation {
            offset,
            target: relocation.target(),
            addend: relocation.addend(),
            width: relocation.width(),
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_elf64(
    target: CraneliftTarget,
    plan: &ObjectModulePlan,
    text: &[u8],
    text_alignment: u64,
    functions: &BTreeMap<DefId, EmittedFunction>,
    labels: &BTreeSet<(DefId, u32)>,
    text_relocs: &[PendingTextRelocation],
    data: &GlobalSections,
) -> Result<Vec<u8>, BackendError> {
    const TEXT_SECTION: u16 = 1;
    const RODATA_SECTION: u16 = 3;
    const DATA_SECTION: u16 = 5;
    const BSS_SECTION: u16 = 7;
    const SYMTAB_SECTION: u32 = 8;
    const STRTAB_SECTION: u32 = 9;
    const SHSTRTAB_SECTION: u16 = 10;
    const SECTION_COUNT: u16 = 12;

    let mut strtab = vec![0u8];
    let mut symbols = vec![ElfSymbol::null()];
    let mut function_symbols = BTreeMap::new();
    let mut global_symbols = BTreeMap::new();
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

    for (owner, symbol) in plan
        .global_symbols()
        .iter()
        .filter(|(_, symbol)| symbol.linkage() == ObjectLinkage::Local)
    {
        let emitted = data.globals.get(owner).ok_or_else(|| {
            object_error(format!("no emitted storage for local global {owner:?}"))
        })?;
        let name = add_string(&mut strtab, symbol.name())?;
        let index = symbols.len() as u32;
        symbols.push(ElfSymbol::object(
            name,
            false,
            data_section_index(emitted.section, RODATA_SECTION, DATA_SECTION, BSS_SECTION),
            emitted.offset,
            emitted.size,
        ));
        global_symbols.insert(*owner, index);
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

    for (owner, symbol) in plan
        .global_symbols()
        .iter()
        .filter(|(_, symbol)| symbol.linkage() == ObjectLinkage::Export)
    {
        let emitted = data.globals.get(owner).ok_or_else(|| {
            object_error(format!("no emitted storage for exported global {owner:?}"))
        })?;
        let name = add_string(&mut strtab, symbol.name())?;
        let index = symbols.len() as u32;
        symbols.push(ElfSymbol::object(
            name,
            true,
            data_section_index(emitted.section, RODATA_SECTION, DATA_SECTION, BSS_SECTION),
            emitted.offset,
            emitted.size,
        ));
        global_symbols.insert(*owner, index);
    }

    if function_symbols.len() != functions.len() {
        return Err(object_error(
            "not every emitted function received an ELF symbol",
        ));
    }
    if global_symbols.len() != data.globals.len() {
        return Err(object_error(
            "not every emitted global received an ELF symbol",
        ));
    }

    let rela_text = encode_text_relocations(
        target,
        text_relocs,
        &function_symbols,
        &label_symbols,
    )?;
    let rela_rodata = encode_data_relocations(
        target,
        &data.rodata_relocs,
        &function_symbols,
        &global_symbols,
    )?;
    let rela_data = encode_data_relocations(
        target,
        &data.data_relocs,
        &function_symbols,
        &global_symbols,
    )?;

    let mut symtab = Vec::with_capacity(symbols.len() * 24);
    for symbol in &symbols {
        symbol.write_to(&mut symtab);
    }

    let mut shstrtab = vec![0u8];
    let text_name = add_string(&mut shstrtab, ".text")?;
    let rela_text_name = add_string(&mut shstrtab, ".rela.text")?;
    let rodata_name = add_string(&mut shstrtab, ".rodata")?;
    let rela_rodata_name = add_string(&mut shstrtab, ".rela.rodata")?;
    let data_name = add_string(&mut shstrtab, ".data")?;
    let rela_data_name = add_string(&mut shstrtab, ".rela.data")?;
    let bss_name = add_string(&mut shstrtab, ".bss")?;
    let symtab_name = add_string(&mut shstrtab, ".symtab")?;
    let strtab_name = add_string(&mut shstrtab, ".strtab")?;
    let shstrtab_name = add_string(&mut shstrtab, ".shstrtab")?;
    let stack_name = add_string(&mut shstrtab, ".note.GNU-stack")?;

    let mut output = vec![0u8; 64];
    let text_offset = append_section(&mut output, text, text_alignment.max(1))?;
    let rela_text_offset = append_section(&mut output, &rela_text, 8)?;
    let rodata_offset = append_section(&mut output, &data.rodata, data.rodata_alignment.max(1))?;
    let rela_rodata_offset = append_section(&mut output, &rela_rodata, 8)?;
    let data_offset = append_section(&mut output, &data.data, data.data_alignment.max(1))?;
    let rela_data_offset = append_section(&mut output, &rela_data, 8)?;
    align_vec(&mut output, data.bss_alignment.max(1))?;
    let bss_offset = output.len() as u64;
    let symtab_offset = append_section(&mut output, &symtab, 8)?;
    let strtab_offset = append_section(&mut output, &strtab, 1)?;
    let shstrtab_offset = append_section(&mut output, &shstrtab, 1)?;
    let stack_offset = output.len() as u64;
    align_vec(&mut output, 8)?;
    let section_headers_offset = output.len() as u64;

    SectionHeader::null().write_to(&mut output);
    SectionHeader::progbits(text_name, 0x6, text_offset, text.len() as u64, text_alignment.max(1))
        .write_to(&mut output);
    SectionHeader::rela(
        rela_text_name,
        rela_text_offset,
        rela_text.len() as u64,
        SYMTAB_SECTION,
        u32::from(TEXT_SECTION),
    )
    .write_to(&mut output);
    SectionHeader::progbits(
        rodata_name,
        0x2,
        rodata_offset,
        data.rodata.len() as u64,
        data.rodata_alignment.max(1),
    )
    .write_to(&mut output);
    SectionHeader::rela(
        rela_rodata_name,
        rela_rodata_offset,
        rela_rodata.len() as u64,
        SYMTAB_SECTION,
        u32::from(RODATA_SECTION),
    )
    .write_to(&mut output);
    SectionHeader::progbits(
        data_name,
        0x3,
        data_offset,
        data.data.len() as u64,
        data.data_alignment.max(1),
    )
    .write_to(&mut output);
    SectionHeader::rela(
        rela_data_name,
        rela_data_offset,
        rela_data.len() as u64,
        SYMTAB_SECTION,
        u32::from(DATA_SECTION),
    )
    .write_to(&mut output);
    SectionHeader::nobits(
        bss_name,
        0x3,
        bss_offset,
        data.bss_size,
        data.bss_alignment.max(1),
    )
    .write_to(&mut output);
    SectionHeader::symtab(
        symtab_name,
        symtab_offset,
        symtab.len() as u64,
        STRTAB_SECTION,
        first_global,
    )
    .write_to(&mut output);
    SectionHeader::strtab(strtab_name, strtab_offset, strtab.len() as u64)
        .write_to(&mut output);
    SectionHeader::strtab(shstrtab_name, shstrtab_offset, shstrtab.len() as u64)
        .write_to(&mut output);
    SectionHeader::progbits(stack_name, 0, stack_offset, 0, 1).write_to(&mut output);

    write_elf_header(
        &mut output[..64],
        target,
        section_headers_offset,
        SECTION_COUNT,
        SHSTRTAB_SECTION,
    );
    Ok(output)
}

fn encode_text_relocations(
    target: CraneliftTarget,
    relocs: &[PendingTextRelocation],
    function_symbols: &BTreeMap<DefId, u32>,
    label_symbols: &BTreeMap<(DefId, u32), u32>,
) -> Result<Vec<u8>, BackendError> {
    let mut output = Vec::with_capacity(relocs.len() * 24);
    for reloc in relocs {
        let symbol = match reloc.target {
            TextRelocationTarget::Function(owner) => {
                *function_symbols.get(&owner).ok_or_else(|| {
                    object_error(format!(
                        "unresolved Forge relocation target {owner:?}; target is not in object plan"
                    ))
                })?
            }
            TextRelocationTarget::Label { owner, offset } => {
                *label_symbols.get(&(owner, offset)).ok_or_else(|| {
                    object_error(format!(
                        "unresolved internal relocation label {owner:?}+{offset}"
                    ))
                })?
            }
        };
        push_rela(
            &mut output,
            reloc.offset,
            symbol,
            elf_relocation_type(target, reloc.kind)?,
            reloc.addend,
        );
    }
    Ok(output)
}

fn encode_data_relocations(
    target: CraneliftTarget,
    relocs: &[PendingDataRelocation],
    function_symbols: &BTreeMap<DefId, u32>,
    global_symbols: &BTreeMap<DefId, u32>,
) -> Result<Vec<u8>, BackendError> {
    let mut output = Vec::with_capacity(relocs.len() * 24);
    for reloc in relocs {
        let symbol = match reloc.target {
            StaticSymbol::Function(owner) => *function_symbols.get(&owner).ok_or_else(|| {
                object_error(format!(
                    "unresolved static function address {owner:?}; target is not in object plan"
                ))
            })?,
            StaticSymbol::Global(owner) => *global_symbols.get(&owner).ok_or_else(|| {
                object_error(format!(
                    "unresolved static global address {owner:?}; target is not in object plan"
                ))
            })?,
        };
        let kind = match reloc.width {
            4 => Reloc::Abs4,
            8 => Reloc::Abs8,
            width => {
                return Err(object_error(format!(
                    "unsupported static-data relocation width {width}"
                )))
            }
        };
        push_rela(
            &mut output,
            reloc.offset,
            symbol,
            elf_relocation_type(target, kind)?,
            reloc.addend,
        );
    }
    Ok(output)
}

fn push_rela(output: &mut Vec<u8>, offset: u64, symbol: u32, kind: u32, addend: i64) {
    push_u64(output, offset);
    push_u64(output, (u64::from(symbol) << 32) | u64::from(kind));
    push_i64(output, addend);
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

fn data_section_index(section: DataSection, rodata: u16, data: u16, bss: u16) -> u16 {
    match section {
        DataSection::ReadOnly => rodata,
        DataSection::Writable => data,
        DataSection::Bss => bss,
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
        Self::typed(name, global, 2, section, value, size)
    }

    const fn object(name: u32, global: bool, section: u16, value: u64, size: u64) -> Self {
        Self::typed(name, global, 1, section, value, size)
    }

    const fn typed(
        name: u32,
        global: bool,
        kind: u8,
        section: u16,
        value: u64,
        size: u64,
    ) -> Self {
        let binding = if global { 1 } else { 0 };
        Self {
            name,
            info: (binding << 4) | kind,
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

    const fn progbits(name: u32, flags: u64, offset: u64, size: u64, alignment: u64) -> Self {
        Self {
            name,
            kind: 1,
            flags,
            offset,
            size,
            link: 0,
            info: 0,
            alignment,
            entry_size: 0,
        }
    }

    const fn nobits(name: u32, flags: u64, offset: u64, size: u64, alignment: u64) -> Self {
        Self {
            name,
            kind: 8,
            flags,
            offset,
            size,
            link: 0,
            info: 0,
            alignment,
            entry_size: 0,
        }
    }

    const fn rela(name: u32, offset: u64, size: u64, link: u32, info: u32) -> Self {
        Self {
            name,
            kind: 4,
            flags: 0,
            offset,
            size,
            link,
            info,
            alignment: 8,
            entry_size: 24,
        }
    }

    const fn symtab(name: u32, offset: u64, size: u64, link: u32, info: u32) -> Self {
        Self {
            name,
            kind: 2,
            flags: 0,
            offset,
            size,
            link,
            info,
            alignment: 8,
            entry_size: 24,
        }
    }

    const fn strtab(name: u32, offset: u64, size: u64) -> Self {
        Self {
            name,
            kind: 3,
            flags: 0,
            offset,
            size,
            link: 0,
            info: 0,
            alignment: 1,
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

fn append_section(output: &mut Vec<u8>, bytes: &[u8], alignment: u64) -> Result<u64, BackendError> {
    align_vec(output, alignment)?;
    let offset = output.len() as u64;
    output.extend_from_slice(bytes);
    Ok(offset)
}

fn align_vec(output: &mut Vec<u8>, alignment: u64) -> Result<(), BackendError> {
    let aligned = align_u64(output.len() as u64, alignment)?;
    let padding = usize::try_from(aligned - output.len() as u64)
        .map_err(|_| object_error("object alignment padding does not fit usize"))?;
    output.resize(output.len() + padding, 0);
    Ok(())
}

fn align_u64(value: u64, alignment: u64) -> Result<u64, BackendError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(object_error(format!(
            "invalid object alignment {alignment}"
        )));
    }
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| object_error("object alignment overflow"))
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
