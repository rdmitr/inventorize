use std::error::Error;
use std::fs::{self, DirEntry, ReadDir};
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};

use crate::util;

/// A recursive directory iterator.
///
/// Unlike `std::fs::ReadDir`, this iterator visits subdirectories of the
/// root directory. Entries for child directories are not returned.
///
/// The files are visited in the depth-first order.
pub struct DirectoryIterator {
    /// Skip hidden files.
    skip_hidden: bool,

    /// Root directory.
    root: PathBuf,

    /// Stack of `std::fs::ReadDir` iterators.
    stack: Vec<ReadDir>,
}

impl DirectoryIterator {
    /// Creates a new recursive directory iterator.
    pub fn new<P: AsRef<Path>>(root: P, skip_hidden: bool) -> IoResult<Self> {
        Ok(DirectoryIterator {
            skip_hidden,
            root: root.as_ref().to_path_buf(),
            // Create the root directory iterator and push it onto the stack.
            stack: vec![fs::read_dir(root)?],
        })
    }

    /// Returns an iterator adapter that produces paths to the discovered files
    /// instead of the `std::fs::DirEntry` directory entries.
    pub fn paths(self) -> impl Iterator<Item = IoResult<PathBuf>> {
        self.map(|r| r.map(|e| e.path()))
    }

    /// Returns an iterator adapter that produces file paths relative to the
    /// root directory of the iterator.
    pub fn relative_paths(self) -> impl Iterator<Item = Result<PathBuf, Box<dyn Error>>> {
        let root = self.root.clone();
        RelativePathIterator::new(self.paths(), root)
    }

    /// Descends into a subdirectory with the given path.
    fn descend<P: AsRef<Path>>(&mut self, subdir: P) -> IoResult<()> {
        // Create the subdirectory iterator and push it onto the stack.
        let iter = fs::read_dir(subdir)?;
        self.stack.push(iter);
        Ok(())
    }

    /// Advances the iterator to the next directory entry, descending into
    /// a subdirectory if needed.
    ///
    /// If the end of the directory is reached, returns `None`. Never ascends
    /// to the parent directory.
    fn step(&mut self) -> Option<IoResult<DirEntry>> {
        debug_assert!(self.stack.len() > 0);

        let skip_hidden = self.skip_hidden;

        // Skip any hidden files if configured to do so.
        let mut iterator = self.stack.last_mut().unwrap().skip_while(|r| match r {
            Ok(e) => skip_hidden && util::is_hidden(e.path()),
            Err(_) => false,
        });

        // Get the next directory entry and return it if it is a file, or
        // descend into the subdirectory if it is a directory.
        match iterator.next() {
            Some(dir_result) => match dir_result {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_dir() {
                        // Try to descend into the subdirectory and start
                        // iterating over its entries.
                        match self.descend(path) {
                            Ok(_) => self.step(),
                            Err(err) => Some(Err(err)),
                        }
                    } else {
                        // A file entry was found, return it.
                        Some(Ok(entry))
                    }
                }
                Err(err) => Some(Err(err)),
            },
            None => None,
        }
    }
}

impl Iterator for DirectoryIterator {
    type Item = IoResult<DirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        debug_assert!(self.stack.len() > 0);

        while self.stack.len() > 0 {
            let result = self.step();
            if result.is_some() {
                return result;
            } else {
                self.stack.pop().unwrap();
            }
        }

        None
    }
}

/// An adapter for a directory iterator that produces file paths relative to
/// some root directory.
struct RelativePathIterator<I> {
    /// The underlying directory iterator.
    iter: I,

    /// Root directory to produce relative paths.
    root: PathBuf,
}

impl<I: Iterator<Item = IoResult<PathBuf>>> RelativePathIterator<I> {
    /// Creates a new relative path iterator.
    fn new(iter: I, root: PathBuf) -> Self {
        RelativePathIterator { iter, root }
    }
}

impl<I: Iterator<Item = IoResult<PathBuf>>> Iterator for RelativePathIterator<I> {
    type Item = Result<PathBuf, Box<dyn Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        // Advance the underlying directory iterator and try to produce the
        // relative path to the discovered entry.
        Some(match self.iter.next()? {
            Ok(p) => match p.strip_prefix(&self.root) {
                Ok(p) => Ok(p.to_path_buf()),
                Err(e) => Err(Box::new(e)),
            },
            Err(e) => Err(Box::new(e)),
        })
    }
}
