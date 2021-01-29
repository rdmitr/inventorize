use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt::Debug;
use std::fs::{self, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use log::debug;

use serde::{Deserialize, Serialize};

use crate::file_err;
use crate::hash::{HashAlgorithm, HashValue, Hasher};
use crate::iterdir::DirectoryIterator;
use crate::util::FileError;

/// Inventory configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Configuration {
    /// Version of the app used to build the inventory.
    version: String,

    /// Skip hidden files.
    skip_hidden: bool,

    /// Hash algorithms to use.
    hash_algorithms: BTreeSet<HashAlgorithm>,
}

impl Configuration {
    /// Returns an empty configuration.
    pub fn new() -> Self {
        Configuration::default()
    }

    /// Sets the `skip_hidden` mode.
    pub fn set_skip_hidden(&mut self, skip_hidden: bool) -> &mut Self {
        self.skip_hidden = skip_hidden;
        self
    }

    /// Sets the hash algorithms to use.
    pub fn set_hash_algorithms(&mut self, algorithms: &[HashAlgorithm]) {
        self.hash_algorithms.clear();
        self.hash_algorithms.extend(algorithms.iter());
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            version: env!("CARGO_PKG_VERSION").to_string(),
            skip_hidden: false,
            hash_algorithms: BTreeSet::new(),
        }
    }
}

/// An inventory record.
#[derive(Debug, Deserialize, Serialize)]
struct Record {
    /// Hashes of the file.
    hashes: BTreeMap<HashAlgorithm, HashValue>,

    /// Size of the file.
    size: u64,
}

impl Record {
    /// Creates a new inventory record.
    fn new(size: u64, hashes: Vec<(HashAlgorithm, HashValue)>) -> Self {
        Record {
            hashes: hashes.into_iter().collect(),
            size,
        }
    }
}

/// Inventory verification failure kind.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum FailureKind {
    /// A file is present in the inventory but missing from the repository.
    MissingFromRepository,

    /// A file is found in the repository but missing from the inventory.
    MissingFromInventory,

    /// Actual file size does not match the size recorded in the inventory.
    SizeMismatch,

    /// Actual file hash value does not match the value recorded in the inventory.
    HashMismatch,
}

/// Inventory verification report.
#[derive(Default)]
pub struct Report {
    /// Issues found during the verification and the corresponding file paths.
    contents: HashMap<FailureKind, HashSet<PathBuf>>,
}

impl Report {
    /// Returns a new empty report.
    fn new() -> Self {
        Report::default()
    }

    /// Returns `true` if the report is empty, `false` otherwise.
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Returns a list of the discovered failure kinds.
    pub fn failures(&self) -> Vec<FailureKind> {
        self.contents.keys().copied().collect()
    }

    /// Returns a list of files that caused the specific failure.
    pub fn by_failure(&self, kind: FailureKind) -> Option<impl Iterator<Item = &Path>> {
        self.contents
            .get(&kind)
            .map(|h| h.iter().map(|p| p.as_path()))
    }

    /// Records a failure in the report.
    fn add_failure<P: AsRef<Path>>(&mut self, file: P, kind: FailureKind) {
        self.contents
            .entry(kind)
            .or_default()
            .insert(file.as_ref().to_path_buf());
    }
}

/// Inventory structure.
#[derive(Debug, Deserialize, Serialize)]
pub struct Inventory {
    /// Inventory configuration.
    configuration: Configuration,

    /// File records.
    records: BTreeMap<PathBuf, Record>,
}

impl Inventory {
    /// Creates a new inventory with the provided configuration.
    pub fn new(configuration: Configuration) -> Self {
        Inventory {
            records: BTreeMap::new(),
            configuration,
        }
    }

    /// Builds an inventory for the provided repository directory.
    pub fn build(configuration: Configuration, repository: &Path) -> Result<Self, Box<dyn Error>> {
        let mut files =
            DirectoryIterator::new(repository, configuration.skip_hidden)?.relative_paths();

        let mut hasher = Hasher::new(configuration.hash_algorithms.iter().copied());
        let mut inventory = Inventory::new(configuration);

        // Add the discovered files to the inventory.
        files.try_for_each(|r| r.and_then(|p| inventory.add_file(repository, &p, &mut hasher)))?;

        Ok(inventory)
    }

    /// Checks the repository and produces the verification report.
    pub fn check(&self, repository: &Path, check_hashes: bool) -> Result<Report, Box<dyn Error>> {
        let mut hasher = Hasher::new(self.configuration.hash_algorithms.iter().copied());
        let files =
            DirectoryIterator::new(repository, self.configuration.skip_hidden)?.relative_paths();

        // Build a set of repository file paths and a set of file paths recorded in the inventory.
        let repository_files = files.into_iter().collect::<Result<HashSet<_>, _>>()?;
        let inventory_files: HashSet<_> = self.records.keys().cloned().collect();

        let mut report = Report::new();

        // Find files present in the repository but missing from the inventory.
        repository_files
            .difference(&inventory_files)
            .for_each(|p| report.add_failure(p, FailureKind::MissingFromInventory));

        // Find files present in the inventory but missing from the repository.
        inventory_files
            .difference(&repository_files)
            .for_each(|p| report.add_failure(p, FailureKind::MissingFromRepository));

        // Verify files one by one.
        for file in inventory_files.intersection(&repository_files) {
            debug!("Verifying file {:?}", file);

            let rec = self.records.get(file).unwrap();

            // Produce the absolute path to the file.
            let mut file_abs = repository.to_path_buf();
            file_abs.push(file);

            // Check size first. It does not make sense to check hashes if sizes
            // don't match.
            let attr = fs::metadata(&file_abs).or_else(|e| file_err!(&file_abs, e))?;
            if attr.len() != rec.size {
                report.add_failure(file, FailureKind::SizeMismatch);
            } else if check_hashes {
                let reader = BufReader::new(
                    OpenOptions::new()
                        .read(true)
                        .open(&file_abs)
                        .or_else(|e| file_err!(&file_abs, e))?,
                );

                let hashes: BTreeMap<_, _> = hasher.compute(reader)?.into_iter().collect();

                if hashes != rec.hashes {
                    report.add_failure(file, FailureKind::HashMismatch);
                }
            }
        }

        Ok(report)
    }

    /// Updates the inventory by adding new files and removing missing files.
    pub fn update(
        &mut self,
        repository: &Path,
        remove_missing: bool,
    ) -> Result<(), Box<dyn Error>> {
        let mut hasher = Hasher::new(self.configuration.hash_algorithms.iter().copied());
        let files =
            DirectoryIterator::new(repository, self.configuration.skip_hidden)?.relative_paths();

        // Build a set of repository file paths and a set of file paths recorded in the inventory.
        let repository_files = files.into_iter().collect::<Result<HashSet<PathBuf>, _>>()?;
        let inventory_files: HashSet<_> = self.records.keys().cloned().collect();

        // Discover files missing from the inventory and add them.
        repository_files
            .difference(&inventory_files)
            .try_for_each(|p| self.add_file(repository, p, &mut hasher))?;

        // If enabled, remove missing files from the inventory.
        if remove_missing {
            inventory_files.difference(&repository_files).for_each(|p| {
                self.records.remove(p);
            });
        }

        Ok(())
    }

    /// Produces a file record for the specified file and adds it to the inventory.
    fn add_file<P: AsRef<Path>>(
        &mut self,
        repository: P,
        rel_path: P,
        hasher: &mut Hasher,
    ) -> Result<(), Box<dyn Error>> {
        debug!("Adding file {:?}", rel_path.as_ref());

        // Produce the absolute path to the file.
        let mut abs_path = repository.as_ref().to_path_buf();
        abs_path.push(&rel_path);

        let attr = abs_path.metadata().or_else(|e| file_err!(&abs_path, e))?;

        // Create a reader to compute the hash(es) the file contents.
        let reader = BufReader::new(
            OpenOptions::new()
                .read(true)
                .open(&abs_path)
                .or_else(|e| file_err!(&abs_path, e))?,
        );

        let hashes = hasher.compute(reader)?;
        let rec = Record::new(attr.len(), hashes);
        self.records.insert(rel_path.as_ref().to_path_buf(), rec);

        Ok(())
    }
}
