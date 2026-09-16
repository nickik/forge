use crate::{
    link_sia32_sectioned_objects, BackendError, Sia32Object, Sia32Section, Sia32SectionBases,
};

pub const SIA32_TEXT_ALIGNMENT: u32 = 2;
pub const SIA32_DATA_ALIGNMENT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sia32ImageRange {
    pub address: u32,
    pub size: u32,
}

impl Sia32ImageRange {
    pub fn end(self) -> Result<u32, BackendError> {
        self.address
            .checked_add(self.size)
            .ok_or_else(|| image_error("SIA32 image range exceeds 32-bit address space"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sia32ImageLayout {
    pub load_address: u32,
    pub text: Sia32ImageRange,
    pub rodata: Sia32ImageRange,
    pub data: Sia32ImageRange,
    pub bss: Sia32ImageRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sia32ExecutableImage {
    bytes: Vec<u8>,
    entry: u32,
    layout: Sia32ImageLayout,
}

impl Sia32ExecutableImage {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn entry(&self) -> u32 {
        self.entry
    }

    pub fn layout(&self) -> Sia32ImageLayout {
        self.layout
    }
}

/// Link SIAO32 objects into the deterministic M8 flat executable image.
///
/// Layout is `.text`, `.rodata`, `.data`, `.bss`; text is 2-byte aligned and
/// all data sections are 4-byte aligned. Object fragments are kept in input
/// order inside each section. BSS occupies zero-filled bytes in the flat image
/// so the image can be loaded directly into the SIA32 reference simulator.
pub fn build_sia32_flat_image(
    objects: &[Sia32Object],
    load_address: u32,
    entry_symbol: &str,
    bss_size: u32,
) -> Result<Sia32ExecutableImage, BackendError> {
    let mut cursor = align_address(load_address, SIA32_TEXT_ALIGNMENT)?;
    if cursor != load_address {
        return Err(image_error(
            "SIA32 flat image load address must be 2-byte aligned",
        ));
    }

    let text_address = cursor;
    let mut bases = vec![
        Sia32SectionBases {
            text: 0,
            rodata: 0,
            data: 0
        };
        objects.len()
    ];
    for (index, object) in objects.iter().enumerate() {
        cursor = align_address(cursor, SIA32_TEXT_ALIGNMENT)?;
        bases[index].text = cursor;
        cursor = add_len(cursor, object.section(Sia32Section::Text).len(), "text")?;
    }
    let text_end = cursor;

    cursor = align_address(cursor, SIA32_DATA_ALIGNMENT)?;
    let rodata_address = cursor;
    for (index, object) in objects.iter().enumerate() {
        cursor = align_address(cursor, SIA32_DATA_ALIGNMENT)?;
        bases[index].rodata = cursor;
        cursor = add_len(cursor, object.section(Sia32Section::Rodata).len(), "rodata")?;
    }
    let rodata_end = cursor;

    cursor = align_address(cursor, SIA32_DATA_ALIGNMENT)?;
    let data_address = cursor;
    for (index, object) in objects.iter().enumerate() {
        cursor = align_address(cursor, SIA32_DATA_ALIGNMENT)?;
        bases[index].data = cursor;
        cursor = add_len(cursor, object.section(Sia32Section::Data).len(), "data")?;
    }
    let data_end = cursor;

    cursor = align_address(cursor, SIA32_DATA_ALIGNMENT)?;
    let bss_address = cursor;
    let image_end = cursor
        .checked_add(bss_size)
        .ok_or_else(|| image_error("SIA32 BSS exceeds 32-bit address space"))?;

    let entry = resolve_symbol(objects, &bases, entry_symbol)?;
    if entry < text_address || entry >= text_end {
        return Err(image_error(format!(
            "SIA32 entry symbol {entry_symbol:?} is not in executable text"
        )));
    }
    if entry & 1 != 0 {
        return Err(image_error(format!(
            "SIA32 entry symbol {entry_symbol:?} is not 2-byte aligned"
        )));
    }

    let linked = link_sia32_sectioned_objects(objects, &bases)?;
    let image_len = image_end
        .checked_sub(load_address)
        .ok_or_else(|| image_error("SIA32 image end precedes load address"))?;
    let mut bytes = vec![
        0;
        usize::try_from(image_len).map_err(|_| {
            image_error("SIA32 image length does not fit host address space")
        })?
    ];

    for (index, object) in linked.iter().enumerate() {
        copy_section(&mut bytes, load_address, bases[index].text, &object.text)?;
        copy_section(
            &mut bytes,
            load_address,
            bases[index].rodata,
            &object.rodata,
        )?;
        copy_section(&mut bytes, load_address, bases[index].data, &object.data)?;
    }

    let layout = Sia32ImageLayout {
        load_address,
        text: Sia32ImageRange {
            address: text_address,
            size: text_end - text_address,
        },
        rodata: Sia32ImageRange {
            address: rodata_address,
            size: rodata_end - rodata_address,
        },
        data: Sia32ImageRange {
            address: data_address,
            size: data_end - data_address,
        },
        bss: Sia32ImageRange {
            address: bss_address,
            size: bss_size,
        },
    };

    Ok(Sia32ExecutableImage {
        bytes,
        entry,
        layout,
    })
}

fn resolve_symbol(
    objects: &[Sia32Object],
    bases: &[Sia32SectionBases],
    name: &str,
) -> Result<u32, BackendError> {
    let mut found = None;
    for (object, base) in objects.iter().zip(bases) {
        let Some(symbol) = object.symbols().get(name) else {
            continue;
        };
        if found.is_some() {
            return Err(image_error(format!(
                "duplicate SIA32 entry symbol {name:?}"
            )));
        }
        let section_base = match symbol.section() {
            Sia32Section::Text => base.text,
            Sia32Section::Rodata => base.rodata,
            Sia32Section::Data => base.data,
        };
        found = Some(section_base.checked_add(symbol.offset()).ok_or_else(|| {
            image_error(format!("SIA32 entry symbol {name:?} address overflows"))
        })?);
    }
    found.ok_or_else(|| image_error(format!("unresolved SIA32 entry symbol {name:?}")))
}

fn align_address(value: u32, alignment: u32) -> Result<u32, BackendError> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| image_error("SIA32 image alignment exceeds 32-bit address space"))
}

fn add_len(address: u32, len: usize, section: &str) -> Result<u32, BackendError> {
    let len = u32::try_from(len)
        .map_err(|_| image_error(format!("SIA32 {section} exceeds 32-bit address space")))?;
    address
        .checked_add(len)
        .ok_or_else(|| image_error(format!("SIA32 {section} placement overflows")))
}

fn copy_section(
    image: &mut [u8],
    load_address: u32,
    address: u32,
    section: &[u8],
) -> Result<(), BackendError> {
    let offset = address
        .checked_sub(load_address)
        .ok_or_else(|| image_error("SIA32 section precedes image load address"))?;
    let start = usize::try_from(offset)
        .map_err(|_| image_error("SIA32 section offset does not fit host address space"))?;
    let end = start
        .checked_add(section.len())
        .ok_or_else(|| image_error("SIA32 section copy overflows host address space"))?;
    let destination = image
        .get_mut(start..end)
        .ok_or_else(|| image_error("SIA32 section lies outside flat image"))?;
    destination.copy_from_slice(section);
    Ok(())
}

fn image_error(message: impl Into<String>) -> BackendError {
    BackendError::Cranelift {
        message: message.into(),
    }
}
