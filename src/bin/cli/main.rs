mod gemini;
mod mailing_list;

use crate::mailing_list::MailingList;
use clap::Parser;

#[derive(Parser)]
#[command(name = "cli")]
#[command(about = "Lawh", long_about = None)]
struct Cli {
    #[arg(
        short = 'd',
        long = "outdir",
        value_name = "DIR",
        help = "Output directory"
    )]
    outdir_path: String,

    #[arg(
        short = 'i',
        long = "indir",
        value_name = "DIR",
        help = "Input directory (i.e., the maildir path)"
    )]
    indir_path: String,

    #[arg(
        long = "inboxes",
        value_name = "PATHS",
        help = "Comma-separated list of specific maildir paths relative to indir (e.g., Folders/discuss@dev.kazimi.io,Folders/patches@dev.kazimi.io)"
    )]
    inboxes: Option<String>,
}

// Check if a directory is a maildir by looking for cur/ or new/ subdirectories
fn is_maildir(path: &std::path::Path) -> bool {
    path.join("cur").is_dir() || path.join("new").is_dir()
}

// Recursively collect all maildirs from a directory
// If the directory itself is a maildir, return it
// Otherwise, recurse into subdirectories looking for maildirs
//
// We're expecting a mailbox entry here, like:
// /my-maildir
// ├── patches
// │   ├── cur
// │   ├── new
// │   └── tmp
// ├── discuss
// │   ├── cur
// │   ├── new
// │   └── tmp
// └── Folders
//     └── nested-list
//         ├── cur
//         ├── new
//         └── tmp
fn collect_maildirs(path: &std::path::Path, results: &mut Vec<std::path::PathBuf>) {
    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Skip hidden directories and the standard maildir subdirs
    if dir_name.starts_with('.') || ["cur", "new", "tmp"].contains(&dir_name) {
        return;
    }

    if is_maildir(path) {
        results.push(path.to_path_buf());
    } else if path.is_dir() {
        // Not a maildir, recurse into children
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                collect_maildirs(&entry.path(), results);
            }
        }
    }
}

fn main() {
    // Init logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Parse args
    let cli = Cli::parse();
    log::info!("Arg: Output Directory: {}", cli.outdir_path);
    log::info!("Arg: Input Directory: {}", cli.indir_path);
    if let Some(ref inboxes) = cli.inboxes {
        log::info!("Arg: Inboxes: {}", inboxes);
    }

    let indir = std::path::Path::new(&cli.indir_path);

    // Collect maildirs based on whether --inboxes was specified
    let maildir_paths: Vec<std::path::PathBuf> = if let Some(ref inboxes) = cli.inboxes {
        // User specified specific inboxes, validate each one exists and is a maildir
        let paths: Vec<std::path::PathBuf> = inboxes
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|relative_path| {
                let full_path = indir.join(relative_path);
                if !full_path.exists() {
                    panic!("Inbox path does not exist: {:?}", full_path);
                }
                if !is_maildir(&full_path) {
                    panic!(
                        "Path is not a valid maildir (missing cur/ or new/): {:?}",
                        full_path
                    );
                }
                full_path
            })
            .collect();

        if paths.is_empty() {
            panic!("No valid inbox paths provided in --inboxes argument");
        }
        paths
    } else {
        // No specific inboxes specified, collect all maildirs recursively
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        for entry in std::fs::read_dir(indir).expect("reading maildir input directory") {
            let entry = entry.expect("reading directory entry");
            collect_maildirs(&entry.path(), &mut paths);
        }
        paths
    };

    let mut lists: Vec<MailingList> = Vec::new();
    for maildir_path in maildir_paths {
        log::info!("Working with list: {:?}", maildir_path);
        let mailing_list =
            MailingList::from_maildir_path(&maildir_path).expect("parsing maildir");
        log::info!(
            "Parsed {} threads with {} total messages",
            mailing_list.threads.len(),
            mailing_list.id_to_index.len()
        );
        lists.push(mailing_list);
    }
    log::info!("Total mailing lists: {}", lists.len());

    // Nuke and mkdir output directory if it doesn't exist
    let _ = std::fs::remove_dir_all(&cli.outdir_path);
    std::fs::create_dir_all(&cli.outdir_path).unwrap();

    gemini::make_gemini_site(&lists, &cli.outdir_path).expect("generating gemini output");
}
