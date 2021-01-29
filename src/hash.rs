use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::{Error as IoError, Read};
use std::iter::Iterator;
use std::str::FromStr;

use digest::{Digest, DynDigest};
use md5::Md5;
use sha1::Sha1;

use serde::{Deserialize, Serialize};

use crate::util;

/// MD5 hash algorithm name.
const NAME_MD5: &str = "md5";

/// SHA1 hash algorithm name.
const NAME_SHA1: &str = "sha1";

/// An error returned when the hash algorithm name cannot be parsed.
#[derive(Debug)]
pub struct ParseHashAlgorithmError();

impl Display for ParseHashAlgorithmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Invalid algorithm name")
    }
}

/// Hash algorithm identifier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "&str")]
pub enum HashAlgorithm {
    /// MD5 hash algorithm.
    Md5,

    /// SHA1 hash algorithm.
    Sha1,
}

impl TryFrom<&str> for HashAlgorithm {
    type Error = ParseHashAlgorithmError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            NAME_MD5 => Ok(HashAlgorithm::Md5),
            NAME_SHA1 => Ok(HashAlgorithm::Sha1),
            _ => Err(ParseHashAlgorithmError()),
        }
    }
}

impl TryFrom<String> for HashAlgorithm {
    type Error = ParseHashAlgorithmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        HashAlgorithm::try_from(value.as_str())
    }
}

impl FromStr for HashAlgorithm {
    type Err = ParseHashAlgorithmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HashAlgorithm::try_from(s)
    }
}

impl From<HashAlgorithm> for &str {
    fn from(a: HashAlgorithm) -> Self {
        match a {
            HashAlgorithm::Md5 => NAME_MD5,
            HashAlgorithm::Sha1 => NAME_SHA1,
        }
    }
}

/// An error returned when the hash value cannot be parsed.
#[derive(Debug)]
pub struct ParseHashValueError();

impl Display for ParseHashValueError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Invalid hash value hex string")
    }
}

/// A hash value produced by a hash algorithm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HashValue(Box<[u8]>);

impl From<Box<[u8]>> for HashValue {
    fn from(b: Box<[u8]>) -> Self {
        HashValue(b)
    }
}

impl TryFrom<&str> for HashValue {
    type Error = ParseHashValueError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let b = util::hex_string_to_bytes(value).ok_or(ParseHashValueError())?;
        Ok(b.into())
    }
}

impl TryFrom<String> for HashValue {
    type Error = ParseHashValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        HashValue::try_from(value.as_str())
    }
}

impl From<HashValue> for String {
    fn from(value: HashValue) -> Self {
        util::bytes_to_hex_string(&value.0)
    }
}

/// A hasher that contains one or more hash algorithms.
pub struct Hasher {
    /// A list of digest algorithm implementations and their identifiers.
    digests: Vec<(HashAlgorithm, Box<dyn DynDigest>)>,
}

impl Hasher {
    /// Creates a new hasher with a given set of hash algorithm implementations.
    pub fn new<A: Iterator<Item = HashAlgorithm>>(algorithms: A) -> Self {
        let digests: Vec<_> = algorithms
            .map(|a| {
                let d: Box<dyn DynDigest> = match a {
                    HashAlgorithm::Md5 => Box::new(Md5::new()),
                    HashAlgorithm::Sha1 => Box::new(Sha1::new()),
                };
                (a, d)
            })
            .collect();

        debug_assert!(!digests.is_empty());

        Hasher { digests }
    }

    /// Updates all contained digests with a chunk of data.
    fn update(&mut self, data: &[u8]) {
        self.digests.iter_mut().for_each(|(_, d)| d.update(data));
    }

    /// Finalizes the computation and resets the digests.
    ///
    /// Returns the produced hash values.
    fn finalize_reset(&mut self) -> Vec<(HashAlgorithm, HashValue)> {
        self.digests
            .iter_mut()
            .map(|(a, d)| (*a, d.finalize_reset().into()))
            .collect()
    }

    /// Computes the hashes of data returned by the specified reader.
    pub fn compute<R: Read>(
        &mut self,
        mut source: R,
    ) -> Result<Vec<(HashAlgorithm, HashValue)>, IoError> {
        const CHUNK_SIZE: usize = 128 * 1024;
        let mut buf = [0u8; CHUNK_SIZE];

        // Read data in chunks and update the digests.
        loop {
            let nread = source.read(&mut buf[..])?;
            if nread > 0 {
                self.update(&buf[..nread]);
            } else {
                break;
            }
        }

        Ok(self.finalize_reset())
    }
}
