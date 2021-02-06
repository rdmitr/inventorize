use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{
    crate_authors, crate_version, value_t_or_exit, values_t_or_exit, App, AppSettings, Arg,
    SubCommand,
};

use serde_json;

use env_logger::{self, Builder as LogBuilder};
use log::{self, error, info, LevelFilter};

mod hash;
mod inventory;
mod iterdir;
mod util;

use hash::HashAlgorithm;
use inventory::{Configuration, FailureKind, Inventory};
use util::FileError;

/// High-level errors returned by the application.
#[derive(Debug)]
enum AppError {
    InventoryExists(PathBuf),
    VerificationFailed,
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match &self {
            AppError::InventoryExists(path) => {
                write!(f, "Inventory file exists: {:?}", path)
            }
            AppError::VerificationFailed => {
                write!(f, "Verification failed")
            }
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self {
            AppError::InventoryExists(_) => None,
            AppError::VerificationFailed => None,
        }
    }
}

/// Arguments of the `build` subcommand.
struct CommandBuild {
    /// Overwrite the inventory file if present.
    overwrite: bool,

    /// Skip hidden files in the repository.
    skip_hidden: bool,

    /// Hash algorithms to use.
    hash_algorithms: Vec<HashAlgorithm>,
}

/// Arguments of the `verify` subcommand.
struct CommandVerify {
    /// Quick verification mode (only file presence and their sizes are checked).
    quick: bool,
}

/// Arguments of the `update` subcommand.
struct CommandUpdate {
    /// Remove missing files from the inventory.
    remove_missing: bool,
}

/// Supported subcommands and their arguments.
enum Command {
    /// The `build` subcommand.
    Build(CommandBuild),

    /// The `verify` subcommand.
    Verify(CommandVerify),

    /// The `update` subcommand.
    Update(CommandUpdate),
}

/// Common command-line options.
struct Options {
    /// Verbosity level.
    verbosity: usize,

    /// Path to the inventory file.
    inventory: PathBuf,

    /// Path to the repository.
    repository: PathBuf,
}

/// Application parameters specified on the command line.
struct Parameters {
    /// Common options.
    options: Options,

    /// Subcommand and options.
    command: Command,
}

/// Builds the inventory file.
fn build(options: Options, command: CommandBuild) -> Result<(), Box<dyn Error>> {
    // Check that the inventory exists before computing the hashes which can
    // take quite a while.
    if options.inventory.exists() && !command.overwrite {
        return Err(Box::new(AppError::InventoryExists(options.inventory)));
    }

    // Initialize the configuration and build the inventory.
    let mut inventory_config = Configuration::new();
    inventory_config.set_skip_hidden(command.skip_hidden);
    inventory_config.set_hash_algorithms(command.hash_algorithms.as_slice());
    let inventory = Inventory::build(inventory_config, &options.repository)?;

    // Serialize the inventory to the JSON file.
    let inventory_writer = BufWriter::new(
        OpenOptions::new()
            .create(command.overwrite)
            .create_new(!command.overwrite)
            .truncate(command.overwrite)
            .write(true)
            .open(&options.inventory)
            .or_else(|e| file_err!(&options.inventory, e))?,
    );
    serde_json::to_writer_pretty(inventory_writer, &inventory)?;

    info!("Inventory built successfully.");

    Ok(())
}

/// Verifies the repository using a pre-built inventory.
fn verify(options: Options, command: CommandVerify) -> Result<(), Box<dyn Error>> {
    // Open the inventory file for reading.
    let inventory_file = OpenOptions::new()
        .read(true)
        .open(&options.inventory)
        .or_else(|e| file_err!(&options.inventory, e))?;
    let inventory_reader = BufReader::new(inventory_file);
    let inventory: Inventory = serde_json::from_reader(inventory_reader)?;

    // Check the inventory and produce the report.
    let report = inventory.check(&options.repository, !command.quick)?;

    // Output the issues, if any.
    for failure in report.failures() {
        let descr = match failure {
            FailureKind::MissingFromRepository => "Missing from repository",
            FailureKind::MissingFromInventory => "Missing from inventory",
            FailureKind::SizeMismatch => "Size mismatch",
            FailureKind::HashMismatch => "Hash mismatch",
        };

        let sorted: BTreeSet<_> = report.by_failure(failure).unwrap().collect();
        for file in sorted {
            error!("{}: {:?}", descr, file);
        }
    }

    if !report.is_empty() {
        Err(Box::new(AppError::VerificationFailed))
    } else {
        info!("No issues found.");
        Ok(())
    }
}

/// Updates the inventory with files added to or removed from the repository.
fn update(options: Options, command: CommandUpdate) -> Result<(), Box<dyn Error>> {
    // Open the inventory file for reading.
    let inventory_file = OpenOptions::new()
        .read(true)
        .open(&options.inventory)
        .or_else(|e| file_err!(&options.inventory, e))?;
    let inventory_reader = BufReader::new(inventory_file);
    let mut inventory: Inventory = serde_json::from_reader(inventory_reader)?;

    // Update the inventory in-place.
    inventory.update(&options.repository, command.remove_missing)?;

    // Serialize the inventory to the JSON file.
    let inventory_writer = BufWriter::new(
        OpenOptions::new()
            .truncate(true)
            .write(true)
            .open(&options.inventory)
            .or_else(|e| file_err!(&options.inventory, e))?,
    );
    serde_json::to_writer_pretty(inventory_writer, &inventory)?;

    info!("Inventory updated successfully.");

    Ok(())
}

/// Executes the subcommand specified by the caller.
fn run(parameters: Parameters) -> Result<(), Box<dyn Error>> {
    match parameters.command {
        Command::Build(command) => build(parameters.options, command),
        Command::Verify(command) => verify(parameters.options, command),
        Command::Update(command) => update(parameters.options, command),
    }
}

/// Canonicalizes the inventory file path.
///
/// The inventory file is not required to exist, since not every subcommand
/// needs it (e.g. `build`). This means that it cannot be canonicalized using
/// the `canonicalize()` method.
///
/// However, the inventory's parent directory must exist. It can also be empty,
/// which means that the current working directory is to be used (if e.g.
/// "inventory.json" is specified as the inventory argument).
fn canonicalize_inventory_path<P: AsRef<Path>>(inventory: P) -> Result<PathBuf, String> {
    let inventory = inventory.as_ref();
    let (dir_path, file_name) = (inventory.parent(), inventory.file_name());

    if dir_path.is_none() || file_name.is_none() {
        return Err(String::from("inventory filename not specified"));
    }

    let (dir_path, file_name) = (dir_path.unwrap(), file_name.unwrap());

    let dir_path = if dir_path.as_os_str().len() == 0 {
        Path::new(".")
    } else {
        dir_path
    };

    let mut ret = dir_path
        .canonicalize()
        .map_err(|_| "inventory directory is inaccessible or does not exist".to_string())?;
    ret.push(file_name);

    Ok(ret)
}

/// Parses the command line arguments.
///
/// Prints an error message and exits the application if the command-line
/// configuration is invalid.
fn parse_cmd_line<I, T>(args: I) -> Parameters
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    const DEFAULT_REPOSITORY_DIR: &str = ".";
    const DEFAULT_HASH_ALGORITHM: &str = "md5";

    let matches = App::new("inventorize")
        .about("Builds and maintains an inventory of files in a repository directory")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequired)
        .arg(
            Arg::with_name("verbose")
                .help("Verbose output")
                .long("verbose")
                .multiple(true),
        )
        .arg(
            Arg::with_name("inventory")
                .help("Path to the inventory file (must be outside of the repository)")
                .long("inventory")
                .number_of_values(1)
                .required(true)
                .validator(|s| canonicalize_inventory_path(PathBuf::from(s)).and_then(|_| Ok(()))),
        )
        .arg(
            Arg::with_name("repository")
                .default_value(DEFAULT_REPOSITORY_DIR)
                .help("Path to the repository")
                .long("repository")
                .number_of_values(1)
                .validator(|s| {
                    let p = PathBuf::from(s);
                    if !p.exists() {
                        Err("repository does not exist".to_string())
                    } else if !p.is_dir() {
                        Err("repository is not a directory".to_string())
                    } else if p.canonicalize().is_err() {
                        Err("cannot canonicalize repository path".to_string())
                    } else {
                        Ok(())
                    }
                }),
        )
        .subcommand(
            SubCommand::with_name("build")
                .about("Builds the inventory")
                .arg(
                    Arg::with_name("overwrite")
                        .help("Overwrite inventory file if it exists")
                        .long("overwrite"),
                )
                .arg(
                    Arg::with_name("skip-hidden")
                        .help("Skip hidden files")
                        .long("skip-hidden"),
                )
                .arg(
                    Arg::with_name("hash-algorithm")
                        .default_value(DEFAULT_HASH_ALGORITHM)
                        .help("Hash algorithm(s) to use")
                        .long("hash-algorithm")
                        .multiple(true)
                        .number_of_values(1)
                        .validator(|s| {
                            HashAlgorithm::from_str(&s)
                                .and(Ok(()))
                                .or(Err("invalid algorithm name".to_string()))
                        }),
                ),
        )
        .subcommand(
            SubCommand::with_name("verify").about("Verifies files").arg(
                Arg::with_name("quick")
                    .help("Quick verification")
                    .long("quick"),
            ),
        )
        .subcommand(
            SubCommand::with_name("update")
                .about("Updates the inventory")
                .arg(
                    Arg::with_name("remove-missing")
                        .help("Remove missing files from inventory")
                        .long("remove-missing"),
                ),
        )
        .get_matches_from(args);

    // Extract the subcommand-specific options.
    let command = match matches.subcommand() {
        ("build", Some(matches)) => Command::Build(CommandBuild {
            overwrite: matches.is_present("overwrite"),
            skip_hidden: matches.is_present("skip-hidden"),
            hash_algorithms: values_t_or_exit!(matches, "hash-algorithm", HashAlgorithm),
        }),
        ("verify", Some(matches)) => Command::Verify(CommandVerify {
            quick: matches.is_present("quick"),
        }),
        ("update", Some(matches)) => Command::Update(CommandUpdate {
            remove_missing: matches.is_present("remove-missing"),
        }),
        _ => unreachable!(),
    };

    // Both inventory and repository paths have been validated and thus can be
    // canonicalized safely.
    let inventory =
        canonicalize_inventory_path(&value_t_or_exit!(matches, "inventory", PathBuf)).unwrap();
    let repository = value_t_or_exit!(matches, "repository", PathBuf)
        .canonicalize()
        .unwrap();

    if inventory.starts_with(&repository) {
        eprintln!("error: inventory must be located outside of the repository");
        std::process::exit(1);
    }

    Parameters {
        options: Options {
            verbosity: matches.occurrences_of("verbose") as usize,
            inventory,
            repository,
        },
        command,
    }
}

/// Initializes the global logger.
fn init_logging(verbosity: usize) {
    let mut builder = LogBuilder::new();

    let level = match verbosity {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    builder
        .filter_level(level)
        .format_module_path(false)
        .format_timestamp_millis()
        .init();
}

fn main() {
    let parameters = parse_cmd_line(env::args());
    init_logging(parameters.options.verbosity);

    std::process::exit(match run(parameters) {
        Ok(_) => 0,
        Err(err) => {
            error!("{}", err);
            1
        }
    });
}
