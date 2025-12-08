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

    let mut lists: Vec<MailingList> = Vec::new();
    for maildir in std::fs::read_dir(&cli.indir_path).expect("reading maildir input directory") {
        // We're expecting a mailbox entry here, like:
        // /my-maildir
        // ├── patches
        // ├────── cur
        // ├────── new
        // ├────── tmp
        // ├── discuss
        // ├────── cur
        // ├────── new
        // ├────── tmp
        let maildir = maildir.expect("reading maildir entry");
        let dir_name = maildir.file_name().into_string().unwrap();
        // Skip cur, new, tmp directories and hidden files
        if dir_name.starts_with('.') || ["cur", "new", "tmp"].contains(&dir_name.as_str()) {
            continue;
        }
        log::info!("Working with list: {:?}", maildir.path());
        let mailing_list =
            MailingList::from_maildir_path(&maildir.path()).expect("parsing maildir");
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
