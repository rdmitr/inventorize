use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::Error as IoError;
use std::path::{Path, PathBuf};

/// Produces a new `Err(FileError)` with the given `std::io::Error` and
/// the file path.
#[macro_export]
macro_rules! file_err {
    ($p:expr, $e:expr) => {
        Err(FileError::new($p, $e))
    };
}

/// An error structure that holds the underlying `std::io::Error`, as well as
/// the path to the file that caused the error.
#[derive(Debug)]
pub struct FileError {
    /// Path to the file that caused the error.
    path: PathBuf,

    /// Underlying `std::io::Error`.
    io_err: IoError,
}

impl FileError {
    /// Creates a new `FileError`.
    pub fn new<P: AsRef<Path>>(path: P, io_err: IoError) -> Self {
        FileError {
            path: path.as_ref().to_path_buf(),
            io_err,
        }
    }
}

impl Display for FileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "File I/O error: {:?}: {}", self.path, self.io_err)
    }
}

impl Error for FileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.io_err)
    }
}

/// Returns an ASCII character representing the provided nibble in hex.
fn nibble_to_char(n: u8) -> u8 {
    debug_assert!(n <= 15);

    match n {
        0..=9 => b'0' + n,
        _ => b'a' + n - 10,
    }
}

/// Returns the numeric value of the provided hexadecimal character.
///
/// Supports both upper- and lower-case representations.
fn char_to_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + c - b'a'),
        b'A'..=b'F' => Some(10 + c - b'A'),
        _ => None,
    }
}

/// Converts a byte slice to the hexadecimal string representation.
pub fn bytes_to_hex_string(b: &[u8]) -> String {
    // Allocate an uninitialized vector of bytes.
    // The length of the returned string is exactly twice the length of the
    // input slice, since each byte is represented by two characters.
    let mut ret = Vec::<u8>::with_capacity(b.len() * 2);
    let mut p = ret.as_mut_ptr();

    unsafe {
        // Convert bytes to their hex string representation, nibble by nibble.
        b.iter().for_each(|x| {
            p.write(nibble_to_char(x >> 4));
            p = p.add(1);
            p.write(nibble_to_char(x & 0x0f));
            p = p.add(1);
        });

        ret.set_len(b.len() * 2);

        // `ret` only contains ASCII characters and is safe to convert to UTF-8.
        String::from_utf8_unchecked(ret)
    }
}

/// Converts a string of hexadecimal values to bytes.
pub fn hex_string_to_bytes(s: &str) -> Option<Box<[u8]>> {
    // The length of the string must be even, since each byte is represented
    // by two hexadecimal characters.
    // There is no need to check that the string is pure ASCII, since
    // `char_to_nibble()` will return `None` if it stumbles upon a character
    // that it cannot decode, including non-ASCII UTF-8 code points.
    let len = s.len();
    if len % 2 != 0 {
        return None;
    }

    // Allocate an uninitialized vector of bytes.
    let mut ret = Vec::<u8>::with_capacity(len / 2);
    let mut p = ret.as_mut_ptr();
    let mut it = s.bytes();

    unsafe {
        // Read characters of the input string and convert them to nibbles,
        // then produce bytes from each pair of nibbles.
        loop {
            // Read the high-order nibble character.
            let hi = it.next();
            if let Some(hi) = hi {
                // The length of the input string is guaranteed to be even,
                // so it can be safely assumed that the low-order nibble
                // character is present.
                // Immediately return `None` if a nibble fails to decode.
                let lo = it.next().unwrap();
                p.write(char_to_nibble(hi)? << 4 | char_to_nibble(lo)?);
                p = p.add(1);
            } else {
                // End of the string has been reached.
                break;
            }
        }

        ret.set_len(len / 2);
    }

    Some(ret.into_boxed_slice())
}

/// Checks if a file specified by path is considered hidden.
///
/// Currently, only Unix-specific hidden files are supported (i.e. those
/// whose names start with a dot).
pub fn is_hidden<P: AsRef<Path>>(path: P) -> bool {
    path.as_ref()
        .file_name()
        .and_then(|p| p.to_str())
        .map_or(false, |s| s.starts_with("."))
}
