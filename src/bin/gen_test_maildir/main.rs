use anyhow::Result;
use clap::Parser;
use lettre::message::{header::ContentType, Message};
use rand::seq::SliceRandom;
use rand::Rng;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "gen_test_maildir")]
#[command(about = "Generate test maildir with realistic emails")]
struct Cli {
    #[arg(
        short = 'o',
        long = "outdir",
        value_name = "DIR",
        help = "Output directory for the maildir",
        default_value = "/tmp/test-maildir"
    )]
    outdir: String,

    #[arg(
        short = 'n',
        long = "num-emails",
        value_name = "NUM",
        help = "Number of emails to generate per list",
        default_value = "20"
    )]
    num_emails: usize,
}

// Sample data for generating realistic-looking emails
const NAMES: &[&str] = &[
    "heyhey",
    "byebyebye",
    "ohoh",
    "heyooooo",
    "EEEE",
    "FFFF",
    "GGGG",
    "HHHH",
];

const DOMAINS: &[&str] = &["timbers.ai", "chasez.io", "bass.com", "fatone.cc"];

// Thread subjects that can have replies
const PATCH_SUBJECTS: &[&str] = &[
    "im doing this tonight",
    "youre probably gonna start a fight",
    "i know this cant be right",
    "hey baby cmon",
    "i loved you endlessly",
    "when you werent there for me",
    "now its time to leave and make it alone",
];

const DISCUSS_SUBJECTS: &[&str] = &[
    "i know that i cant take no more",
    "it aint no lie",
    "i want to see you out that door",
    "baby bye bye bye",
];

const BODY_PARAGRAPHS: &[&str] = &[
    "dont wanna be a fool for you just another player in your game for two you may hate me but it aint no lie baby bye bye bye",
    "dont really wanna make it tough i just wanna tell you that ive had enough it might sound crazy but it aint no lie baby bye bye bye",
    "every little thing i do never seems enough for you you dont wanna lose it again but im not like them baby when you finally get to love somebody guess what its gonna be me",
    "this i promise you i will be right here forever i know deep inside i will be the one to make the change who i was isnt who i wanna be",
];

// Returns a formatted email address string like "aaa bbb <aaa.bbb@dev.dev.io>"
fn random_email_address(rng: &mut impl Rng) -> String {
    let name = *NAMES.choose(rng).unwrap();
    let domain = *DOMAINS.choose(rng).unwrap();
    // Convert name to email format: "aaa bbb" -> "aaa.bbb"
    let local_part = name.to_lowercase().replace(' ', ".");
    format!("{} <{}@{}>", name, local_part, domain)
}

// Generate a plain body (no flowed formatting)
fn random_body_plain(rng: &mut impl Rng) -> String {
    let num_paragraphs = rng.gen_range(1..=3);
    let paragraphs: Vec<&str> = BODY_PARAGRAPHS
        .choose_multiple(rng, num_paragraphs)
        .copied()
        .collect();
    paragraphs.join("\n\n")
}

// Generate a format=flowed body
// In format=flowed, lines ending with space are "soft" breaks that should be joined
// Lines without trailing space are "hard" breaks
fn random_body_flowed(rng: &mut impl Rng) -> String {
    let num_paragraphs = rng.gen_range(1..=3);
    let paragraphs: Vec<&str> = BODY_PARAGRAPHS
        .choose_multiple(rng, num_paragraphs)
        .copied()
        .collect();

    // For each paragraph, split into ~40 char chunks with trailing spaces (flowed)
    // except for the last chunk which has no trailing space (fixed/end of paragraph)
    let mut result = String::new();
    for (i, para) in paragraphs.iter().enumerate() {
        if i > 0 {
            result.push_str("\n\n");
        }

        let words: Vec<&str> = para.split_whitespace().collect();
        let mut line = String::new();
        for (j, word) in words.iter().enumerate() {
            if !line.is_empty() && line.len() + word.len() > 40 {
                // End this line with trailing space (flowed)
                result.push_str(&line);
                result.push_str(" \n");
                line = String::new();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);

            // Last word of paragraph: no trailing space
            if j == words.len() - 1 {
                result.push_str(&line);
            }
        }
    }
    result
}

// Generate a SystemTime in the past (within last 30 days)
fn random_system_time(rng: &mut impl Rng) -> SystemTime {
    let days_ago = rng.gen_range(0..30);
    let hours_ago = rng.gen_range(0..24);
    let secs_ago = (days_ago * 86400) + (hours_ago * 3600);
    SystemTime::now() - Duration::from_secs(secs_ago)
}

// Create the standard maildir subdirectories
fn create_maildir_structure(base_path: &PathBuf, list_name: &str) -> Result<PathBuf> {
    let list_path = base_path.join(list_name);
    for subdir in &["cur", "new", "tmp"] {
        fs::create_dir_all(list_path.join(subdir))?;
    }
    Ok(list_path)
}

fn generate_emails(
    list_path: &PathBuf,
    list_name: &str,
    subjects: &[&str],
    num_emails: usize,
) -> Result<()> {
    let mut rng = rand::thread_rng();

    // Keep track of thread subjects and their message IDs for generating replies
    let mut threads: Vec<(String, String)> = Vec::new();

    for i in 0..num_emails {
        let from = random_email_address(&mut rng);
        let to = format!("{}@lists.example.com", list_name);
        let date = random_system_time(&mut rng);

        // Decide if this is a new thread or a reply
        // After a few emails, 50% chance of being a reply
        let (subject, references, in_reply_to) =
            if i > 3 && rng.gen_bool(0.5) && !threads.is_empty() {
                let (original_subject, ref_msg_id) = threads.choose(&mut rng).unwrap().clone();
                // Add "Re: " prefix if not already present
                let reply_subject = if original_subject.starts_with("Re: ") {
                    original_subject
                } else {
                    format!("Re: {}", original_subject)
                };
                (reply_subject, Some(ref_msg_id.clone()), Some(ref_msg_id))
            } else {
                let subject = (*subjects.choose(&mut rng).unwrap()).to_string();
                (subject, None, None)
            };

        // 50% chance of using format=flowed
        let use_flowed = rng.gen_bool(0.5);
        let body = if use_flowed {
            random_body_flowed(&mut rng)
        } else {
            random_body_plain(&mut rng)
        };

        // Generate a unique message ID
        // lettre will auto-generate one if we pass None, but we want to track it for replies
        let msg_id = format!("{}.{}@example.com", Uuid::new_v4(), list_name);

        // Set Content-Type with format=flowed if applicable
        let content_type = if use_flowed {
            ContentType::parse("text/plain; charset=utf-8; format=flowed").unwrap()
        } else {
            ContentType::TEXT_PLAIN
        };

        // Build the email using lettre's MessageBuilder
        let mut builder = Message::builder()
            .from(from.parse().unwrap())
            .to(to.parse().unwrap())
            .subject(&subject)
            .date(date)
            .message_id(Some(msg_id.clone()))
            .header(content_type);

        // Add threading headers if this is a reply
        if let Some(ref ref_id) = references {
            builder = builder.references(ref_id.clone());
        }
        if let Some(ref reply_id) = in_reply_to {
            builder = builder.in_reply_to(reply_id.clone());
        }

        let email = builder.body(body)?;

        // Track this thread for potential future replies
        threads.push((subject, msg_id));

        // Get the raw email bytes
        let email_bytes = email.formatted();

        // Determine if this goes to "new" or "cur"
        // 70% go to cur (already seen), 30% to new
        let (subdir, filename) = if rng.gen_bool(0.7) {
            // cur format: unique_id:2,flags
            // S = Seen flag
            let unique_id = Uuid::new_v4();
            let filename = format!("{}:2,S", unique_id);
            ("cur", filename)
        } else {
            // new format: just unique_id
            let unique_id = Uuid::new_v4();
            ("new", unique_id.to_string())
        };

        let filepath = list_path.join(subdir).join(&filename);
        fs::write(&filepath, &email_bytes)?;

        log::info!("Created email: {}/{}", subdir, filename);
    }

    Ok(())
}

fn main() -> Result<()> {
    // Init logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let cli = Cli::parse();
    let base_path = PathBuf::from(&cli.outdir);

    // Nuke and recreate output directory
    let _ = fs::remove_dir_all(&base_path);
    fs::create_dir_all(&base_path)?;

    log::info!("Generating test maildir at: {}", cli.outdir);

    // Create patches mailing list
    let patches_path = create_maildir_structure(&base_path, "patches")?;
    generate_emails(&patches_path, "patches", PATCH_SUBJECTS, cli.num_emails)?;
    log::info!("Created {} emails in patches list", cli.num_emails);

    // Create discuss mailing list
    let discuss_path = create_maildir_structure(&base_path, "discuss")?;
    generate_emails(&discuss_path, "discuss", DISCUSS_SUBJECTS, cli.num_emails)?;
    log::info!("Created {} emails in discuss list", cli.num_emails);

    log::info!("Test maildir created successfully at: {}", cli.outdir);

    Ok(())
}
