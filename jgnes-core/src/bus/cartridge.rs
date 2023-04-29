mod mappers;

use crate::bus::cartridge::mappers::{
    Axrom, ChrType, Cnrom, Mmc1, Mmc3, NametableMirroring, Nrom, Uxrom,
};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::{cmp, io};
use thiserror::Error;

use crate::bus::PpuWriteToggle;
#[cfg(test)]
pub(crate) use mappers::new_mmc1;

#[derive(Debug, Clone)]
struct Cartridge {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct MapperImpl<MapperData> {
    cartridge: Cartridge,
    data: MapperData,
}

#[derive(Debug, Clone)]
pub(crate) enum Mapper {
    Nrom(MapperImpl<Nrom>),
    Uxrom(MapperImpl<Uxrom>),
    Mmc1(MapperImpl<Mmc1>),
    Cnrom(MapperImpl<Cnrom>),
    Mmc3(MapperImpl<Mmc3>),
    Axrom(MapperImpl<Axrom>),
}

impl Mapper {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Nrom(..) => "NROM",
            Self::Uxrom(..) => "UxROM",
            Self::Mmc1(..) => "MMC1",
            Self::Cnrom(..) => "CNROM",
            Self::Mmc3(..) => "MMC3",
            Self::Axrom(..) => "AxROM",
        }
    }

    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match self {
            Self::Nrom(nrom) => nrom.read_cpu_address(address),
            Self::Uxrom(uxrom) => uxrom.read_cpu_address(address),
            Self::Mmc1(mmc1) => mmc1.read_cpu_address(address),
            Self::Cnrom(cnrom) => cnrom.read_cpu_address(address),
            Self::Mmc3(mmc3) => mmc3.read_cpu_address(address),
            Self::Axrom(axrom) => axrom.read_cpu_address(address),
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match self {
            Self::Nrom(..) => {}
            Self::Uxrom(uxrom) => {
                uxrom.write_cpu_address(address, value);
            }
            Self::Mmc1(mmc1) => {
                mmc1.write_cpu_address(address, value);
            }
            Self::Cnrom(cnrom) => {
                cnrom.write_cpu_address(address, value);
            }
            Self::Mmc3(mmc3) => {
                mmc3.write_cpu_address(address, value);
            }
            Self::Axrom(axrom) => {
                axrom.write_cpu_address(address, value);
            }
        }
    }

    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        match self {
            Self::Nrom(nrom) => nrom.read_ppu_address(address, vram),
            Self::Uxrom(uxrom) => uxrom.read_ppu_address(address, vram),
            Self::Mmc1(mmc1) => mmc1.read_ppu_address(address, vram),
            Self::Cnrom(cnrom) => cnrom.read_ppu_address(address, vram),
            Self::Mmc3(mmc3) => mmc3.read_ppu_address(address, vram),
            Self::Axrom(axrom) => axrom.read_ppu_address(address, vram),
        }
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match self {
            Self::Nrom(nrom) => {
                nrom.write_ppu_address(address, value, vram);
            }
            Self::Uxrom(uxrom) => {
                uxrom.write_ppu_address(address, value, vram);
            }
            Self::Mmc1(mmc1) => {
                mmc1.write_ppu_address(address, value, vram);
            }
            Self::Cnrom(cnrom) => {
                cnrom.write_ppu_address(address, value, vram);
            }
            Self::Mmc3(mmc3) => {
                mmc3.write_ppu_address(address, value, vram);
            }
            Self::Axrom(axrom) => {
                axrom.write_ppu_address(address, value, vram);
            }
        }
    }

    pub(crate) fn tick(&mut self) {
        match self {
            Self::Nrom(..)
            | Self::Uxrom(..)
            | Self::Cnrom(..)
            | Self::Axrom(..)
            | Self::Mmc1(..) => {}
            Self::Mmc3(mmc3) => {
                mmc3.tick();
            }
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        if let Self::Mmc1(mmc1) = self {
            mmc1.tick_cpu();
        }
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        match self {
            Self::Mmc3(mmc3) => mmc3.interrupt_flag(),
            _ => false,
        }
    }

    pub(crate) fn process_ppu_addr_update(&mut self, value: u8, write_toggle: PpuWriteToggle) {
        if let Self::Mmc3(mmc3) = self {
            mmc3.process_ppu_addr_update(value, write_toggle);
        }
    }

    pub(crate) fn process_ppu_addr_increment(&mut self, new_ppu_addr: u16) {
        if let Self::Mmc3(mmc3) = self {
            mmc3.process_ppu_addr_increment(new_ppu_addr);
        }
    }
}

#[derive(Debug, Error)]
pub enum CartridgeFileError {
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: io::Error,
    },
    #[error("invalid or unsupported file format")]
    Format,
    #[error("unsupported mapper: {mapper_number}")]
    UnsupportedMapper { mapper_number: u8 },
    #[error("cartridge header specifies both volatile and non-volatile PRG RAM")]
    MultiplePrgRamTypes,
}

pub(crate) fn from_file<P>(path: P) -> Result<Mapper, CartridgeFileError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();

    log::info!("Loading cartridge from {}", path.display());

    let mut file = File::open(path)?;

    let mut buf = [0; 8];
    file.read_exact(&mut buf)?;

    // First 4 bytes should be equal to "NES<EOF>"
    if buf[..4] != [0x4E, 0x45, 0x53, 0x1A] {
        log::error!("First 4 bytes of file do not match the iNES header");
        return Err(CartridgeFileError::Format);
    }

    from_ines_file(file)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFormat {
    INes,
    Nes2Point0,
}

fn from_ines_file(mut file: File) -> Result<Mapper, CartridgeFileError> {
    file.seek(SeekFrom::Start(0))?;

    let mut header = [0; 16];
    file.read_exact(&mut header)?;

    let prg_rom_size = 16 * 1024 * ((u32::from(header[9] & 0x0F) << 8) | u32::from(header[4]));
    let chr_rom_size = 8 * 1024 * ((u32::from(header[9] & 0xF0) << 4) | u32::from(header[5]));

    let has_trainer = header[6] & 0x04 != 0;

    let mapper_number = (header[7] & 0xF0) | ((header[6] & 0xF0) >> 4);

    let prg_rom_start_address = if has_trainer { 16 + 512 } else { 16 };

    let mut prg_rom = vec![0; prg_rom_size as usize];
    file.seek(SeekFrom::Start(prg_rom_start_address))?;
    file.read_exact(&mut prg_rom)?;

    let mut chr_rom = vec![0; chr_rom_size as usize];
    file.read_exact(&mut chr_rom)?;

    let chr_type = if chr_rom_size == 0 {
        ChrType::RAM
    } else {
        ChrType::ROM
    };

    let nametable_mirroring = if header[6] & 0x01 != 0 {
        NametableMirroring::Vertical
    } else {
        NametableMirroring::Horizontal
    };

    let format = if header[7] & 0x0C == 0x08 {
        FileFormat::Nes2Point0
    } else {
        FileFormat::INes
    };

    let sub_mapper_number = match format {
        FileFormat::Nes2Point0 => header[8] >> 4,
        FileFormat::INes => 0,
    };

    let prg_ram_size = match format {
        FileFormat::Nes2Point0 => {
            let volatile_shift = header[10] & 0x0F;
            let non_volatile_shift = header[10] >> 4;
            if volatile_shift > 0 && non_volatile_shift > 0 {
                // ???
                return Err(CartridgeFileError::MultiplePrgRamTypes);
            }
            let shift = cmp::max(volatile_shift, non_volatile_shift);
            if shift > 0 {
                64 << shift
            } else {
                0
            }
        }
        FileFormat::INes => 8192,
    };

    let chr_ram_size = match (chr_type, format) {
        (ChrType::RAM, FileFormat::Nes2Point0) => {
            let chr_ram_shift = header[11] & 0x0F;
            if chr_ram_shift > 0 {
                64 << chr_ram_shift
            } else {
                0
            }
        }
        (ChrType::RAM, FileFormat::INes) => 8192,
        (ChrType::ROM, _) => 0,
    };

    let chr_size = match chr_type {
        ChrType::ROM => chr_rom.len(),
        ChrType::RAM => chr_ram_size,
    };

    let cartridge = Cartridge {
        prg_rom,
        prg_ram: vec![0; prg_ram_size],
        chr_rom,
        chr_ram: vec![0; chr_ram_size],
    };

    let mapper = match mapper_number {
        0 => Mapper::Nrom(MapperImpl {
            cartridge,
            data: Nrom::new(chr_type, nametable_mirroring),
        }),
        1 => Mapper::Mmc1(MapperImpl {
            cartridge,
            data: Mmc1::new(chr_type),
        }),
        2 => Mapper::Uxrom(MapperImpl {
            cartridge,
            data: Uxrom::new(chr_type, nametable_mirroring),
        }),
        3 => Mapper::Cnrom(MapperImpl {
            cartridge,
            data: Cnrom::new(chr_type, nametable_mirroring),
        }),
        4 => Mapper::Mmc3(MapperImpl {
            cartridge,
            data: Mmc3::new(chr_type, prg_rom_size, chr_size as u32, sub_mapper_number),
        }),
        7 => Mapper::Axrom(MapperImpl {
            cartridge,
            data: Axrom::new(chr_type),
        }),
        _ => {
            return Err(CartridgeFileError::UnsupportedMapper { mapper_number });
        }
    };

    log::info!("Mapper number: {mapper_number} ({})", mapper.name());
    log::info!("PRG ROM size: {prg_rom_size}");
    log::info!("PRG RAM size: {prg_ram_size}");
    log::info!("CHR ROM size: {chr_rom_size}");
    log::info!("CHR RAM size: {chr_ram_size}");
    log::info!("CHR memory type: {chr_type:?}");
    log::info!(
        "Hardwired nametable mirroring: {nametable_mirroring:?} (not applicable to all mappers)"
    );

    Ok(mapper)
}
