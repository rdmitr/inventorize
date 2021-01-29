# inventorize

`inventorize` is a command-line tool for data integrity verification.

In a nutshell, `inventorize` builds an *inventory* of files in a directory
(the *repository*). The inventory contains the paths and the hashes of the
files. Later, `inventorize` can be used to compare the inventory records with
the actual contents of the repository and verify the integrity of the files by
comparing their hashes with the hashes stored in the inventory.

## Quick start guide

Using `inventorize` boils down to three operations:

* Building the inventory file;
* Periodically verifying the contents of the repository using the inventory
  file;
* Updating the inventory when new files are added to the repository, or old ones
  are removed (this does not recompute the hashes of the existing inventory
  records).

The inventory file must not be stored inside the repository directory.

For the detailed description of the available command-line options and commands,
see the reference below.

### Building the inventory

Use the `build` subcommand to build the inventory:

    inventorize
        --repository /path/to/the/repo
        --inventory /path/to/the/inventory.json
        build

### Verifying the contents of the repository

Use the `verify` subcommand to verify the contents of the repository using an
existing inventory file:

    inventorize
        --repository /path/to/the/repo
        --inventory /path/to/the/inventory.json
        verify

### Updating the inventory

Use the `update` subcommand to update the inventory when files are added to the
repository:

    inventorize
        --repository /path/to/the/repo
        --inventory /path/to/the/inventory.json
        update

## Reference

### Command-line options accepted by all subcommands

* `--repository`: path to the repository (defaults to the current working
  directory).
* `--inventory`: path to the inventory file.
* `--verbose`: verbose mode.

### `build` subcommand

The `build` subcommand is used to build the inventory.

By default this subcommand will include hidden files in the inventory. It will
report an error if the inventory file exists.

The default hash algorithm is `md5`. Particularly paranoid users can select
multiple hash algorithms:

    --hash-algorithm=md5 --hash-algorithm=sha1

Supported options:

* `--overwrite`: overwrite the inventory file if it exists.
* `--skip-hidden`: do not include hidden files in the inventory.
* `--hash-algorithm=<ALG>`: hash algorithm to use.

Supported hash algorithms:

* `md5`
* `sha1`

### `verify` subcommand

The `verify` subcommand is used to verify the repository contents using an
existing inventory. An error is returned if any missing, added, or changed files
are found.

By default, all hash values contained in the inventory are checked.
Alternatively, the *quick mode* can be enabled to only check the presence of
files and their sizes. Needless to say, this mode should not be considered a
reliable integrity check.

Supported options:

* `--quick` quick mode: only check presence of files and their sizes.

### `update` subcommand

The `update` subcommand updates the inventory with files added to the repository
after the inventory has been built. It **never** recomputes the hashes of the
existing inventory records.

Files that are gone from the repository but still present in the inventory are
**not** removed from the inventory by default.

Supported options:

* `--remove-missing`: remove files that are no longer found in the repository
  from the inventory.

## Inventory file format

The inventory is stored as a JSON file that contains a list of *records*
(file paths and their hash values), as well as metadata (version of the
application used to build the inventory, and `build` subcommand options).
