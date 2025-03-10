use super::FileAttribute;
use crate::data_types::{chars::NUL_16, Align};
use crate::table::runtime::Time;
use crate::{unsafe_guid, CStr16, Char16, Identify};
use core::cmp;
use core::ffi::c_void;
use core::mem;
use core::slice;

/// Common trait for data structures that can be used with
/// `File::set_info()` or `File::get_info()`.
///
/// The long-winded name is needed because "FileInfo" is already taken by UEFI.
pub trait FileProtocolInfo: Align + Identify + FromUefi {}

/// Trait for going from an UEFI-originated pointer to a Rust reference
///
/// This is trivial for `Sized` types, but requires some work when operating on
/// dynamic-sized types like `NamedFileProtocolInfo`, as the second member of
/// the fat pointer must be reconstructed using hidden UEFI-provided metadata.
pub trait FromUefi {
    /// Turn an UEFI-provided pointer-to-base into a (possibly fat) Rust reference
    ///
    /// # Safety
    ///
    /// This function can lead to undefined behavior if the given pointer is not
    /// pointing to a valid object of the specified type.
    unsafe fn from_uefi<'ptr>(ptr: *mut c_void) -> &'ptr mut Self;
}

/// Dynamically sized `FileProtocolInfo` with a header and an UCS-2 name
///
/// All structs that can currently be queried via Get/SetInfo can be described
/// as a (possibly empty) header followed by a variable-sized name.
///
/// Since such dynamic-sized types are a bit unpleasant to handle in Rust today,
/// this generic struct was created to deduplicate the relevant code.
///
/// The reason why this struct covers the whole DST, as opposed to the
/// `[Char16]` part only, is that pointers to DSTs are created in a rather
/// unintuitive way that is best kept centralized in one place.
#[derive(Debug)]
#[repr(C)]
pub struct NamedFileProtocolInfo<Header> {
    header: Header,
    name: [Char16],
}

impl<Header> NamedFileProtocolInfo<Header> {
    /// Create a `NamedFileProtocolInfo` structure in user-provided storage
    ///
    /// The structure will be created in-place within the provided storage
    /// buffer. The buffer must be large enough to hold the data structure,
    /// including a null-terminated UCS-2 `name` string.
    ///
    /// The buffer must be correctly aligned. You can query the required
    /// alignment using the `alignment()` method of the `Align` trait that this
    /// struct implements.
    #[allow(clippy::cast_ptr_alignment)]
    fn new_impl<'buf>(
        storage: &'buf mut [u8],
        header: Header,
        name: &CStr16,
    ) -> core::result::Result<&'buf mut Self, FileInfoCreationError> {
        // Make sure that the storage is properly aligned
        Self::assert_aligned(storage);

        // Make sure that the storage is large enough for our needs
        let name_length_ucs2 = name.as_slice_with_nul().len();
        let name_size = name_length_ucs2 * mem::size_of::<Char16>();
        let info_size = mem::size_of::<Header>() + name_size;
        if storage.len() < info_size {
            return Err(FileInfoCreationError::InsufficientStorage(info_size));
        }

        // Write the header at the beginning of the storage
        let header_ptr = storage.as_mut_ptr() as *mut Header;
        unsafe {
            header_ptr.write(header);
        }

        // At this point, our storage contains a correct header, followed by
        // random rubbish. It is okay to reinterpret the rubbish as Char16s
        // because 1/we are going to overwrite it and 2/Char16 does not have a
        // Drop implementation. Thus, we are now ready to build a correctly
        // sized &mut Self and go back to the realm of safe code.
        debug_assert!(!mem::needs_drop::<Char16>());
        let info_ptr = unsafe {
            slice::from_raw_parts_mut(storage.as_mut_ptr() as *mut Char16, name_length_ucs2)
                as *mut [Char16] as *mut Self
        };
        let info = unsafe { &mut *info_ptr };
        debug_assert_eq!(info.name.len(), name_length_ucs2);

        // Write down the UCS-2 name before returning the storage reference
        info.name.copy_from_slice(name.as_slice_with_nul());
        debug_assert_eq!(info.name[name_length_ucs2 - 1], NUL_16);

        Ok(info)
    }
}

impl<Header> Align for NamedFileProtocolInfo<Header> {
    fn alignment() -> usize {
        cmp::max(mem::align_of::<Header>(), mem::align_of::<Char16>())
    }
}

impl<Header> FromUefi for NamedFileProtocolInfo<Header> {
    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn from_uefi<'ptr>(ptr: *mut c_void) -> &'ptr mut Self {
        let byte_ptr = ptr as *mut u8;
        let name_ptr = byte_ptr.add(mem::size_of::<Header>()) as *mut Char16;
        let name = CStr16::from_ptr(name_ptr);
        let name_len = name.to_u16_slice_with_nul().len();
        let fat_ptr = slice::from_raw_parts_mut(ptr as *mut Char16, name_len);
        let self_ptr = fat_ptr as *mut [Char16] as *mut Self;
        &mut *self_ptr
    }
}

/// Errors that can occur when creating a `FileProtocolInfo`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileInfoCreationError {
    /// The provided buffer was too small to hold the `FileInfo`. You need at
    /// least the indicated buffer size (in bytes). Please remember that using
    /// a misaligned buffer will cause a decrease of usable storage capacity.
    InsufficientStorage(usize),
}

/// Generic file information
///
/// The following rules apply when using this struct with `set_info()`:
///
/// - On directories, the file size is determined by the contents of the
///   directory and cannot be changed by setting `file_size`. This member is
///   ignored by `set_info()`.
/// - The `physical_size` is determined by the `file_size` and cannot be
///   changed. This member is ignored by `set_info()`.
/// - The `FileAttribute::DIRECTORY` bit cannot be changed. It must match the
///   file’s actual type.
/// - A value of zero in create_time, last_access, or modification_time causes
///   the fields to be ignored (and not updated).
/// - It is forbidden to change the name of a file to the name of another
///   existing file in the same directory.
/// - If a file is read-only, the only allowed change is to remove the read-only
///   attribute. Other changes must be carried out in a separate transaction.
#[unsafe_guid("09576e92-6d3f-11d2-8e39-00a0c969723b")]
pub type FileInfo = NamedFileProtocolInfo<FileInfoHeader>;

/// Header for generic file information
#[derive(Debug)]
#[repr(C)]
pub struct FileInfoHeader {
    size: u64,
    file_size: u64,
    physical_size: u64,
    create_time: Time,
    last_access_time: Time,
    modification_time: Time,
    attribute: FileAttribute,
}

impl FileInfo {
    /// Create a `FileInfo` structure
    ///
    /// The structure will be created in-place within the provided storage
    /// buffer. The buffer must be large enough to hold the data structure,
    /// including a null-terminated UCS-2 `name` string.
    ///
    /// The buffer must be correctly aligned. You can query the required
    /// alignment using the `alignment()` method of the `Align` trait that this
    /// struct implements.
    #[allow(clippy::too_many_arguments)]
    pub fn new<'buf>(
        storage: &'buf mut [u8],
        file_size: u64,
        physical_size: u64,
        create_time: Time,
        last_access_time: Time,
        modification_time: Time,
        attribute: FileAttribute,
        file_name: &CStr16,
    ) -> core::result::Result<&'buf mut Self, FileInfoCreationError> {
        let header = FileInfoHeader {
            size: 0,
            file_size,
            physical_size,
            create_time,
            last_access_time,
            modification_time,
            attribute,
        };
        let info = Self::new_impl(storage, header, file_name)?;
        info.header.size = mem::size_of_val(info) as u64;
        Ok(info)
    }

    /// File size (number of bytes stored in the file)
    pub fn file_size(&self) -> u64 {
        self.header.file_size
    }

    /// Physical space consumed by the file on the file system volume
    pub fn physical_size(&self) -> u64 {
        self.header.physical_size
    }

    /// Time when the file was created
    pub fn create_time(&self) -> &Time {
        &self.header.create_time
    }

    /// Time when the file was last accessed
    pub fn last_access_time(&self) -> &Time {
        &self.header.last_access_time
    }

    /// Time when the file's contents were last modified
    pub fn modification_time(&self) -> &Time {
        &self.header.modification_time
    }

    /// Attribute bits for the file
    pub fn attribute(&self) -> FileAttribute {
        self.header.attribute
    }

    /// Name of the file
    pub fn file_name(&self) -> &CStr16 {
        unsafe { CStr16::from_ptr(&self.name[0]) }
    }
}

impl FileProtocolInfo for FileInfo {}

/// System volume information
///
/// May only be obtained on the root directory's file handle.
///
/// Please note that only the system volume's volume label may be set using
/// this information structure. Consider using `FileSystemVolumeLabel` instead.
#[unsafe_guid("09576e93-6d3f-11d2-8e39-00a0c969723b")]
pub type FileSystemInfo = NamedFileProtocolInfo<FileSystemInfoHeader>;

/// Header for system volume information
#[derive(Debug)]
#[repr(C)]
pub struct FileSystemInfoHeader {
    size: u64,
    read_only: bool,
    volume_size: u64,
    free_space: u64,
    block_size: u32,
}

impl FileSystemInfo {
    /// Create a `FileSystemInfo` structure
    ///
    /// The structure will be created in-place within the provided storage
    /// buffer. The buffer must be large enough to hold the data structure,
    /// including a null-terminated UCS-2 `name` string.
    ///
    /// The buffer must be correctly aligned. You can query the required
    /// alignment using the `alignment()` method of the `Align` trait that this
    /// struct implements.
    #[allow(clippy::too_many_arguments)]
    pub fn new<'buf>(
        storage: &'buf mut [u8],
        read_only: bool,
        volume_size: u64,
        free_space: u64,
        block_size: u32,
        volume_label: &CStr16,
    ) -> core::result::Result<&'buf mut Self, FileInfoCreationError> {
        let header = FileSystemInfoHeader {
            size: 0,
            read_only,
            volume_size,
            free_space,
            block_size,
        };
        let info = Self::new_impl(storage, header, volume_label)?;
        info.header.size = mem::size_of_val(info) as u64;
        Ok(info)
    }

    /// Truth that the volume only supports read access
    pub fn read_only(&self) -> bool {
        self.header.read_only
    }

    /// Number of bytes managed by the file system
    pub fn volume_size(&self) -> u64 {
        self.header.volume_size
    }

    /// Number of available bytes for use by the file system
    pub fn free_space(&self) -> u64 {
        self.header.free_space
    }

    /// Nominal block size by which files are typically grown
    pub fn block_size(&self) -> u32 {
        self.header.block_size
    }

    /// Volume label
    pub fn volume_label(&self) -> &CStr16 {
        unsafe { CStr16::from_ptr(&self.name[0]) }
    }
}

impl FileProtocolInfo for FileSystemInfo {}

/// System volume label
///
/// May only be obtained on the root directory's file handle.
#[unsafe_guid("db47d7d3-fe81-11d3-9a35-0090273fc14d")]
pub type FileSystemVolumeLabel = NamedFileProtocolInfo<FileSystemVolumeLabelHeader>;

/// Header for system volume label information
#[derive(Debug)]
#[repr(C)]
pub struct FileSystemVolumeLabelHeader {}

impl FileSystemVolumeLabel {
    /// Create a `FileSystemVolumeLabel` structure
    ///
    /// The structure will be created in-place within the provided storage
    /// buffer. The buffer must be large enough to hold the data structure,
    /// including a null-terminated UCS-2 `name` string.
    ///
    /// The buffer must be correctly aligned. You can query the required
    /// alignment using the `alignment()` method of the `Align` trait that this
    /// struct implements.
    pub fn new<'buf>(
        storage: &'buf mut [u8],
        volume_label: &CStr16,
    ) -> core::result::Result<&'buf mut Self, FileInfoCreationError> {
        let header = FileSystemVolumeLabelHeader {};
        Self::new_impl(storage, header, volume_label)
    }

    /// Volume label
    pub fn volume_label(&self) -> &CStr16 {
        unsafe { CStr16::from_ptr(&self.name[0]) }
    }
}

impl FileProtocolInfo for FileSystemVolumeLabel {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc_api::vec;
    use crate::table::runtime::{Daylight, Time};
    use crate::CString16;

    #[test]
    fn test_file_info() {
        let mut storage = vec![0; 128];

        let file_size = 123;
        let physical_size = 456;
        let create_time = Time::new(1970, 1, 1, 0, 0, 0, 0, 0, Daylight::IN_DAYLIGHT);
        let last_access_time = Time::new(1971, 1, 1, 0, 0, 0, 0, 0, Daylight::IN_DAYLIGHT);
        let modification_time = Time::new(1972, 1, 1, 0, 0, 0, 0, 0, Daylight::IN_DAYLIGHT);
        let attribute = FileAttribute::READ_ONLY;
        let name = CString16::try_from("test_name").unwrap();
        let info = FileInfo::new(
            &mut storage,
            file_size,
            physical_size,
            create_time,
            last_access_time,
            modification_time,
            attribute,
            &name,
        )
        .unwrap();

        //   Header size: 80 bytes
        // + Name size (including trailing null): 20 bytes
        // = 100
        // Round size up to match FileInfo alignment of 8: 104
        assert_eq!(info.header.size, 104);

        assert_eq!(info.file_size(), file_size);
        assert_eq!(info.physical_size(), physical_size);
        assert_eq!(info.create_time(), &create_time);
        assert_eq!(info.last_access_time(), &last_access_time);
        assert_eq!(info.modification_time(), &modification_time);
        assert_eq!(info.attribute(), attribute);
        assert_eq!(info.file_name(), name);
    }

    #[test]
    fn test_file_system_info() {
        let mut storage = vec![0; 128];

        let read_only = false;
        let volume_size = 123;
        let free_space = 456;
        let block_size = 789;
        let name = CString16::try_from("test_name").unwrap();
        let info = FileSystemInfo::new(
            &mut storage,
            read_only,
            volume_size,
            free_space,
            block_size,
            &name,
        )
        .unwrap();

        //   Header size: 40 bytes
        // + Name size (including trailing null): 20 bytes
        // = 60
        // Round size up to match FileInfo alignment of 8: 64
        assert_eq!(info.header.size, 64);

        assert_eq!(info.read_only(), read_only);
        assert_eq!(info.volume_size(), volume_size);
        assert_eq!(info.free_space(), free_space);
        assert_eq!(info.block_size(), block_size);
        assert_eq!(info.volume_label(), name);
    }

    #[test]
    fn test_file_system_volume_label() {
        let mut storage = vec![0; 128];

        let name = CString16::try_from("test_name").unwrap();
        let info = FileSystemVolumeLabel::new(&mut storage, &name).unwrap();

        assert_eq!(info.volume_label(), name);
    }
}
