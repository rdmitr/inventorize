use std::fs::{self, DirEntry, ReadDir};
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};

/// A recursive directory iterator.
///
/// Unlike `std::fs::ReadDir`, this iterator visits subdirectories of the
/// root directory. Entries for child directories are not returned.
///
/// The files are visited in the depth-first order.
pub struct DirectoryIterator {
    /// Stack of `std::fs::ReadDir` iterators.
    stack: Vec<ReadDir>,
}

impl DirectoryIterator {
    /// Creates a new recursive directory iterator.
    pub fn new<P: AsRef<Path>>(root: P) -> IoResult<Self> {
        Ok(DirectoryIterator {
            // Create the root directory iterator and push it onto the stack.
            stack: vec![fs::read_dir(root)?],
        })
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

        // Get the next directory entry and return it if it is a file, or
        // descend into the subdirectory if it is a directory.
        match self.stack.last_mut().unwrap().next() {
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
pub struct RelativePathIterator {
    /// The underlying directory iterator.
    iter: DirectoryIterator,

    /// Root directory to produce relative paths.
    root: PathBuf,
}

impl RelativePathIterator {
    /// Creates a new relative path iterator.
    pub fn new<P: AsRef<Path>>(root: P) -> IoResult<Self> {
        Ok(RelativePathIterator {
            iter: DirectoryIterator::new(&root)?,
            root: root.as_ref().to_path_buf(),
        })
    }
}

impl Iterator for RelativePathIterator {
    type Item = IoResult<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        // Advance the underlying directory iterator and try to produce the
        // relative path to the discovered entry.
        Some(match self.iter.next()? {
            // The returned value of strip_prefix() must be safe to unwrap,
            // since root is always a prefix of the returned paths.
            Ok(d) => Ok(d.path().strip_prefix(&self.root).unwrap().to_path_buf()),
            Err(e) => Err(e),
        })
    }
}
