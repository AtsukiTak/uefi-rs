//! Configuration table utilities.
//!
//! The configuration table is an array of GUIDs and pointers to extra system tables.
//!
//! For example, it can be used to find the ACPI tables.
//!
//! This module contains the actual entries of the configuration table,
//! as well as GUIDs for many known vendor tables.

#![allow(clippy::unreadable_literal)]

use crate::Guid;
use bitflags::bitflags;
use core::ffi::c_void;

/// Contains a set of GUID / pointer for a vendor-specific table.
///
/// The UEFI standard guarantees each entry is unique.
#[derive(Debug)]
#[repr(C)]
pub struct ConfigTableEntry {
    /// The GUID identifying this table.
    pub guid: Guid,
    /// The starting address of this table.
    ///
    /// Whether this is a physical or virtual address depends on the table.
    pub address: *const c_void,
}
/// Entry pointing to the old ACPI 1 RSDP.
pub const ACPI_GUID: Guid = Guid::from_values(0xeb9d2d30, 0x2d88, 0x11d3, 0x9a16, 0x0090273fc14d);

///Entry pointing to the ACPI 2 RSDP.
pub const ACPI2_GUID: Guid = Guid::from_values(0x8868e871, 0xe4f1, 0x11d3, 0xbc22, 0x0080c73c8881);

/// Entry pointing to the SMBIOS 1.0 table.
pub const SMBIOS_GUID: Guid = Guid::from_values(0xeb9d2d31, 0x2d88, 0x11d3, 0x9a16, 0x0090273fc14d);

/// Entry pointing to the SMBIOS 3.0 table.
pub const SMBIOS3_GUID: Guid =
    Guid::from_values(0xf2fd1544, 0x9794, 0x4a2c, 0x992e, 0xe5bbcf20e394);

/// GUID of the UEFI properties table.
///
/// The properties table is used to provide additional info
/// about the UEFI implementation.
pub const PROPERTIES_TABLE_GUID: Guid =
    Guid::from_values(0x880aaca3, 0x4adc, 0x4a04, 0x9079, 0xb747340825e5);

/// This table contains additional information about the UEFI implementation.
#[repr(C)]
pub struct PropertiesTable {
    /// Version of the UEFI properties table.
    ///
    /// The only valid version currently is 0x10_000.
    pub version: u32,
    /// Length in bytes of this table.
    ///
    /// The initial version's length is 16.
    pub length: u32,
    /// Memory protection attributes.
    pub memory_protection: MemoryProtectionAttribute,
}

bitflags! {
    /// Flags describing memory protection.
    pub struct MemoryProtectionAttribute: usize {
        /// If this bit is set, then the UEFI implementation will mark pages
        /// containing data as non-executable.
        const NON_EXECUTABLE_DATA = 1;
    }
}

/// Hand-off Blocks are used to pass data from the early pre-UEFI environment to the UEFI drivers.
///
/// Most OS loaders or applications should not mess with this.
pub const HAND_OFF_BLOCK_LIST_GUID: Guid =
    Guid::from_values(0x7739f24c, 0x93d7, 0x11d4, 0x9a3a, 0x0090273fc14d);

/// Table used in the early boot environment to record memory ranges.
pub const MEMORY_TYPE_INFORMATION_GUID: Guid =
    Guid::from_values(0x4c19049f, 0x4137, 0x4dd3, 0x9c10, 0x8b97a83ffdfa);

/// Used to identify Hand-off Blocks which store
/// status codes reported during the pre-UEFI environment.
pub const MEMORY_STATUS_CODE_RECORD_GUID: Guid =
    Guid::from_values(0x60cc026, 0x4c0d, 0x4dda, 0x8f41, 0x595fef00a502);

/// Table which provides Driver eXecution Environment services.
pub const DXE_SERVICES_GUID: Guid =
    Guid::from_values(0x5ad34ba, 0x6f02, 0x4214, 0x952e, 0x4da0398e2bb9);

/// LZMA-compressed filesystem.
pub const LZMA_COMPRESS_GUID: Guid =
    Guid::from_values(0xee4e5898, 0x3914, 0x4259, 0x9d6e, 0xdc7bd79403cf);

/// A custom compressed filesystem used by the Tiano UEFI implementation.
pub const TIANO_COMPRESS_GUID: Guid =
    Guid::from_values(0xa31280ad, 0x481e, 0x41b6, 0x95e8, 0x127f4c984779);

/// Pointer to the debug image info table.
pub const DEBUG_IMAGE_INFO_GUID: Guid =
    Guid::from_values(0x49152e77, 0x1ada, 0x4764, 0xb7a2, 0x7afefed95e8b);
