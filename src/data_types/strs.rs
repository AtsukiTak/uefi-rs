use super::chars::{Char16, Char8, NUL_16, NUL_8};
#[cfg(feature = "exts")]
use super::CString16;
use core::fmt;
use core::iter::Iterator;
use core::result::Result;
use core::slice;

/// Errors which can occur during checked `[uN]` -> `CStrN` conversions
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FromSliceWithNulError {
    /// An invalid character was encountered before the end of the slice
    InvalidChar(usize),

    /// A null character was encountered before the end of the slice
    InteriorNul(usize),

    /// The slice was not null-terminated
    NotNulTerminated,
}

/// Error returned by [`CStr16::from_str_with_buf`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FromStrWithBufError {
    /// An invalid character was encountered before the end of the string
    InvalidChar(usize),

    /// A null character was encountered in the string
    InteriorNul(usize),

    /// The buffer is not big enough to hold the entire string and
    /// trailing null character
    BufferTooSmall,
}

/// A Latin-1 null-terminated string
///
/// This type is largely inspired by `std::ffi::CStr`, see the documentation of
/// `CStr` for more details on its semantics.
#[repr(transparent)]
pub struct CStr8([Char8]);

impl CStr8 {
    /// Wraps a raw UEFI string with a safe C string wrapper
    ///
    /// # Safety
    ///
    /// The function will start accessing memory from `ptr` until the first
    /// null byte. It's the callers responsability to ensure `ptr` points to
    /// a valid string, in accessible memory.
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char8) -> &'ptr Self {
        let mut len = 0;
        while *ptr.add(len) != NUL_8 {
            len += 1
        }
        let ptr = ptr as *const u8;
        Self::from_bytes_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1))
    }

    /// Creates a C string wrapper from bytes
    pub fn from_bytes_with_nul(chars: &[u8]) -> Result<&Self, FromSliceWithNulError> {
        let nul_pos = chars.iter().position(|&c| c == 0);
        if let Some(nul_pos) = nul_pos {
            if nul_pos + 1 != chars.len() {
                return Err(FromSliceWithNulError::InteriorNul(nul_pos));
            }
            Ok(unsafe { Self::from_bytes_with_nul_unchecked(chars) })
        } else {
            Err(FromSliceWithNulError::NotNulTerminated)
        }
    }

    /// Unsafely creates a C string wrapper from bytes
    ///
    /// # Safety
    ///
    /// It's the callers responsability to ensure chars is a valid Latin-1
    /// null-terminated string, with no interior null bytes.
    pub unsafe fn from_bytes_with_nul_unchecked(chars: &[u8]) -> &Self {
        &*(chars as *const [u8] as *const Self)
    }

    /// Returns the inner pointer to this C string
    pub fn as_ptr(&self) -> *const Char8 {
        self.0.as_ptr()
    }

    /// Converts this C string to a slice of bytes
    pub fn to_bytes(&self) -> &[u8] {
        let chars = self.to_bytes_with_nul();
        &chars[..chars.len() - 1]
    }

    /// Converts this C string to a slice of bytes containing the trailing 0 char
    pub fn to_bytes_with_nul(&self) -> &[u8] {
        unsafe { &*(&self.0 as *const [Char8] as *const [u8]) }
    }
}

/// An UCS-2 null-terminated string
///
/// This type is largely inspired by `std::ffi::CStr`, see the documentation of
/// `CStr` for more details on its semantics.
#[derive(Eq, PartialEq)]
#[repr(transparent)]
pub struct CStr16([Char16]);

impl CStr16 {
    /// Wraps a raw UEFI string with a safe C string wrapper
    ///
    /// # Safety
    ///
    /// The function will start accessing memory from `ptr` until the first
    /// null byte. It's the callers responsability to ensure `ptr` points to
    /// a valid string, in accessible memory.
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char16) -> &'ptr Self {
        let mut len = 0;
        while *ptr.add(len) != NUL_16 {
            len += 1
        }
        let ptr = ptr as *const u16;
        Self::from_u16_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1))
    }

    /// Creates a C string wrapper from a u16 slice
    ///
    /// Since not every u16 value is a valid UCS-2 code point, this function
    /// must do a bit more validity checking than CStr::from_bytes_with_nul
    pub fn from_u16_with_nul(codes: &[u16]) -> Result<&Self, FromSliceWithNulError> {
        for (pos, &code) in codes.iter().enumerate() {
            match code.try_into() {
                Ok(NUL_16) => {
                    if pos != codes.len() - 1 {
                        return Err(FromSliceWithNulError::InteriorNul(pos));
                    } else {
                        return Ok(unsafe { Self::from_u16_with_nul_unchecked(codes) });
                    }
                }
                Err(_) => {
                    return Err(FromSliceWithNulError::InvalidChar(pos));
                }
                _ => {}
            }
        }
        Err(FromSliceWithNulError::NotNulTerminated)
    }

    /// Unsafely creates a C string wrapper from a u16 slice.
    ///
    /// # Safety
    ///
    /// It's the callers responsability to ensure chars is a valid UCS-2
    /// null-terminated string, with no interior null bytes.
    pub unsafe fn from_u16_with_nul_unchecked(codes: &[u16]) -> &Self {
        &*(codes as *const [u16] as *const Self)
    }

    /// Convert a [`&str`] to a `&CStr16`, backed by a buffer.
    ///
    /// The input string must contain only characters representable with
    /// UCS-2, and must not contain any null characters (even at the end of
    /// the input).
    ///
    /// The backing buffer must be big enough to hold the converted string as
    /// well as a trailing null character.
    ///
    /// # Examples
    ///
    /// Convert the UTF-8 string "ABC" to a `&CStr16`:
    ///
    /// ```
    /// use uefi::CStr16;
    ///
    /// let mut buf = [0; 4];
    /// CStr16::from_str_with_buf("ABC", &mut buf).unwrap();
    /// ```
    pub fn from_str_with_buf<'a>(
        input: &str,
        buf: &'a mut [u16],
    ) -> Result<&'a Self, FromStrWithBufError> {
        let mut index = 0;

        // Convert to UTF-16.
        for c in input.encode_utf16() {
            *buf.get_mut(index)
                .ok_or(FromStrWithBufError::BufferTooSmall)? = c;
            index += 1;
        }

        // Add trailing null character.
        *buf.get_mut(index)
            .ok_or(FromStrWithBufError::BufferTooSmall)? = 0;

        // Convert from u16 to Char16. This checks for invalid UCS-2 chars and
        // interior nulls. The NotNulTerminated case is unreachable because we
        // just added a trailing null character.
        Self::from_u16_with_nul(&buf[..index + 1]).map_err(|err| match err {
            FromSliceWithNulError::InvalidChar(p) => FromStrWithBufError::InvalidChar(p),
            FromSliceWithNulError::InteriorNul(p) => FromStrWithBufError::InteriorNul(p),
            FromSliceWithNulError::NotNulTerminated => unreachable!(),
        })
    }

    /// Returns the inner pointer to this C string
    pub fn as_ptr(&self) -> *const Char16 {
        self.0.as_ptr()
    }

    /// Get the underlying [`Char16`] slice, including the trailing null.
    pub fn as_slice_with_nul(&self) -> &[Char16] {
        &self.0
    }

    /// Converts this C string to a u16 slice
    pub fn to_u16_slice(&self) -> &[u16] {
        let chars = self.to_u16_slice_with_nul();
        &chars[..chars.len() - 1]
    }

    /// Converts this C string to a u16 slice containing the trailing 0 char
    pub fn to_u16_slice_with_nul(&self) -> &[u16] {
        unsafe { &*(&self.0 as *const [Char16] as *const [u16]) }
    }

    /// Returns an iterator over this C string
    pub fn iter(&self) -> CStr16Iter {
        CStr16Iter {
            inner: self,
            pos: 0,
        }
    }

    /// Get the number of bytes in the string (including the trailing null character).
    pub fn num_bytes(&self) -> usize {
        self.0.len() * 2
    }

    /// Writes each [`Char16`] as a [´char´] (4 bytes long in Rust language) into the buffer.
    /// It is up the the implementer of [`core::fmt::Write`] to convert the char to a string
    /// with proper encoding/charset. For example, in the case of [`alloc::string::String`]
    /// all Rust chars (UTF-32) get converted to UTF-8.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let firmware_vendor_c16_str: CStr16 = ...;
    /// // crate "arrayvec" uses stack-allocated arrays for Strings => no heap allocations
    /// let mut buf = arrayvec::ArrayString::<128>::new();
    /// firmware_vendor_c16_str.as_str_in_buf(&mut buf);
    /// log::info!("as rust str: {}", buf.as_str());
    /// ```
    ///
    /// [`alloc::string::String`]: https://doc.rust-lang.org/nightly/alloc/string/struct.String.html
    pub fn as_str_in_buf(&self, buf: &mut dyn core::fmt::Write) -> core::fmt::Result {
        for c16 in self.iter() {
            buf.write_char(char::from(*c16))?;
        }
        Ok(())
    }
}

/// An iterator over `CStr16`.
#[derive(Debug)]
pub struct CStr16Iter<'a> {
    inner: &'a CStr16,
    pos: usize,
}

impl<'a> Iterator for CStr16Iter<'a> {
    type Item = &'a Char16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.inner.0.len() - 1 {
            None
        } else {
            self.pos += 1;
            self.inner.0.get(self.pos - 1)
        }
    }
}

impl fmt::Debug for CStr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CStr16({:?})", &self.0)
    }
}

impl fmt::Display for CStr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for c in self.iter() {
            <Char16 as fmt::Display>::fmt(c, f)?;
        }
        Ok(())
    }
}

#[cfg(feature = "exts")]
impl PartialEq<CString16> for &CStr16 {
    fn eq(&self, other: &CString16) -> bool {
        PartialEq::eq(*self, other.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cstr16_num_bytes() {
        let s = CStr16::from_u16_with_nul(&[65, 66, 67, 0]).unwrap();
        assert_eq!(s.num_bytes(), 8);
    }

    #[test]
    fn test_cstr16_from_str_with_buf() {
        let mut buf = [0; 4];

        // OK: buf is exactly the right size.
        let s = CStr16::from_str_with_buf("ABC", &mut buf).unwrap();
        assert_eq!(s.to_u16_slice_with_nul(), [65, 66, 67, 0]);

        // OK: buf is bigger than needed.
        let s = CStr16::from_str_with_buf("A", &mut buf).unwrap();
        assert_eq!(s.to_u16_slice_with_nul(), [65, 0]);

        // Error: buf is too small.
        assert_eq!(
            CStr16::from_str_with_buf("ABCD", &mut buf).unwrap_err(),
            FromStrWithBufError::BufferTooSmall
        );

        // Error: invalid character.
        assert_eq!(
            CStr16::from_str_with_buf("a😀", &mut buf).unwrap_err(),
            FromStrWithBufError::InvalidChar(1),
        );

        // Error: interior null.
        assert_eq!(
            CStr16::from_str_with_buf("a\0b", &mut buf).unwrap_err(),
            FromStrWithBufError::InteriorNul(1),
        );
    }
}
