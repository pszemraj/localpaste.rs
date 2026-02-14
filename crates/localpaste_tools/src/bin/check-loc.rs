//! Rust source line-count checker for repository policy enforcement.

use clap::Parser;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "check-loc",
    about = "Validate Rust file line-count policy with optional documented exceptions"
)]
struct Args {
    /// Maximum allowed lines per file when no exception is configured.
    #[arg(long, default_value_t = 1000)]
    max_lines: usize,

    /// Watchlist threshold for early warning output.
    #[arg(long, default_value_t = 900)]
    warn_lines: usize,

    /// Exception registry path.
    #[arg(long, default_value = "docs/dev/loc-exceptions.toml")]
    exceptions_file: PathBuf,

    /// Root directory to scan recursively.
    #[arg(long, default_value = "crates")]
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ExceptionRegistry {
    #[serde(default)]
    exceptions: Vec<LocException>,
}

#[derive(Debug, Deserialize)]
struct LocException {
    path: String,
    max_lines: usize,
    reason: String,
}

#[derive(Debug)]
struct FileStat {
    path: String,
    lines: usize,
    allowed_max: usize,
    exception_reason: Option<String>,
}

fn main() {
    let args = Args::parse();

    if let Err(message) = run(args) {
        eprintln!("error: {}", message);
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.warn_lines == 0 {
        return Err("--warn-lines must be greater than zero".to_string());
    }
    if args.max_lines == 0 {
        return Err("--max-lines must be greater than zero".to_string());
    }

    let current_dir = std::env::current_dir().map_err(|err| err.to_string())?;
    let root = current_dir.join(&args.root);
    if !root.exists() {
        return Err(format!(
            "scan root does not exist: {}",
            normalize_path(&args.root)
        ));
    }

    let exception_map = load_exception_map(&current_dir, &args.exceptions_file)?;
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files)?;
    files.sort();

    let mut stats = Vec::with_capacity(files.len());
    let mut referenced_exception_paths = HashSet::new();
    for file in files {
        let rel = file
            .strip_prefix(&current_dir)
            .unwrap_or(file.as_path())
            .to_path_buf();
        let rel_norm = normalize_path(rel.as_path());
        let content = fs::read_to_string(&file)
            .map_err(|err| format!("failed reading {}: {}", rel_norm, err))?;
        let lines = content.lines().count();

        let (allowed_max, reason) = if let Some(exception) = exception_map.get(rel_norm.as_str()) {
            referenced_exception_paths.insert(rel_norm.clone());
            (exception.max_lines, Some(exception.reason.clone()))
        } else {
            (args.max_lines, None)
        };

        stats.push(FileStat {
            path: rel_norm,
            lines,
            allowed_max,
            exception_reason: reason,
        });
    }

    let stale_exceptions: Vec<_> = exception_map
        .keys()
        .filter(|path| !referenced_exception_paths.contains(*path))
        .cloned()
        .collect();
    if !stale_exceptions.is_empty() {
        let mut stale_sorted = stale_exceptions;
        stale_sorted.sort();
        return Err(format!(
            "exception paths do not match any scanned Rust file: {}",
            stale_sorted.join(", ")
        ));
    }

    let mut watchlist: Vec<&FileStat> = stats
        .iter()
        .filter(|stat| stat.lines >= args.warn_lines)
        .collect();
    watchlist.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then(left.path.cmp(&right.path))
    });

    let mut violations: Vec<&FileStat> = stats
        .iter()
        .filter(|stat| stat.lines > stat.allowed_max)
        .collect();
    violations.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then(left.path.cmp(&right.path))
    });

    println!(
        "scanned {} Rust files under {}",
        stats.len(),
        normalize_path(args.root.as_path())
    );

    if watchlist.is_empty() {
        println!("watchlist: none (threshold {})", args.warn_lines);
    } else {
        println!("watchlist (>= {} lines):", args.warn_lines);
        for stat in watchlist {
            if let Some(reason) = stat.exception_reason.as_ref() {
                println!(
                    "  {:>5} {} (max {}, exception: {})",
                    stat.lines, stat.path, stat.allowed_max, reason
                );
            } else {
                println!(
                    "  {:>5} {} (max {})",
                    stat.lines, stat.path, stat.allowed_max
                );
            }
        }
    }

    if violations.is_empty() {
        println!("line-count policy: PASS");
        return Ok(());
    }

    println!("line-count policy: FAIL");
    println!("violations (lines > allowed max):");
    for stat in violations {
        if let Some(reason) = stat.exception_reason.as_ref() {
            println!(
                "  {:>5} {} (max {}, exception: {})",
                stat.lines, stat.path, stat.allowed_max, reason
            );
        } else {
            println!(
                "  {:>5} {} (max {})",
                stat.lines, stat.path, stat.allowed_max
            );
        }
    }

    Err("line-count violations detected".to_string())
}

fn load_exception_map(
    current_dir: &Path,
    exception_file: &Path,
) -> Result<HashMap<String, LocException>, String> {
    let full_path = current_dir.join(exception_file);
    if !full_path.exists() {
        return Err(format!(
            "exception registry missing: {}",
            normalize_path(exception_file)
        ));
    }

    let raw = fs::read_to_string(&full_path).map_err(|err| {
        format!(
            "failed reading exception registry {}: {}",
            normalize_path(exception_file),
            err
        )
    })?;
    let registry: ExceptionRegistry = toml::from_str(raw.as_str()).map_err(|err| {
        format!(
            "failed parsing exception registry {}: {}",
            normalize_path(exception_file),
            err
        )
    })?;

    let mut map = HashMap::new();
    for entry in registry.exceptions {
        let key = normalize_slashes(entry.path.as_str());
        if key.is_empty() {
            return Err("exception path cannot be empty".to_string());
        }
        if !key.ends_with(".rs") {
            return Err(format!(
                "exception path must target a Rust source file: {}",
                key
            ));
        }
        if entry.max_lines == 0 {
            return Err(format!("exception max_lines must be > 0 for {}", key));
        }
        if entry.reason.trim().is_empty() {
            return Err(format!("exception reason cannot be empty for {}", key));
        }
        if map.insert(key.clone(), entry).is_some() {
            return Err(format!("duplicate exception entry for {}", key));
        }
    }

    Ok(map)
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|err| format!("failed reading directory {}: {}", normalize_path(dir), err))?;
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|err| format!("failed reading metadata {}: {}", normalize_path(&path), err))?;
        if metadata.is_dir() {
            collect_rs_files(path.as_path(), out)?;
            continue;
        }
        if metadata.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    normalize_slashes(raw.as_ref())
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}
