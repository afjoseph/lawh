// Gemini template rendering
use crate::mailing_list::{Mail, MailingList, ThreadSummary};
use linkify::{LinkFinder, LinkKind};
use std::collections::HashSet;
use std::path::PathBuf;

const PAGE_SIZE: usize = 50;

// ANSI escape codes for colored diff output
// Lagrange and other Gemini clients support these in preformatted blocks
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_RESET: &str = "\x1b[0m";

// Escape header text by removing newlines
fn h(s: &str) -> String {
    s.replace(&['\r', '\n'], " ")
}

// Checks if a line looks like it's part of a unified diff
fn is_diff_line(line: &str) -> bool {
    line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("@@ ")
        || line.starts_with('+')
        || line.starts_with('-')
        || line.starts_with(' ')
}

// Checks if this body looks like it contains a patch
fn looks_like_patch(body: &str) -> bool {
    body.contains("diff --git") || body.contains("--- a/")
}

// Extracts URLs from text and returns a deduplicated list
fn extract_urls(body: &str) -> Vec<String> {
    let finder = LinkFinder::new();
    let mut urls: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for link in finder.links(body) {
        if link.kind() == &LinkKind::Url {
            let url = link.as_str().to_string();
            if seen.insert(url.clone()) {
                urls.push(url);
            }
        }
    }

    urls
}

// Formats quoted text (lines starting with >) using Gemini's quote syntax
// Collapses nested quotes (>>, >>>) to single > for cleaner output
fn format_quoted_text(body: &str) -> String {
    let mut out = String::new();

    for line in body.lines() {
        let trimmed = line.trim_start();

        // Detect quote lines: starts with >
        if trimmed.starts_with('>') {
            // Strip all existing > markers and leading whitespace
            let content = trimmed.trim_start_matches('>').trim_start_matches(' ');
            // Use Gemini quote syntax
            out.push_str(&format!("> {}\r\n", content));
        } else if trimmed.starts_with("On ") && trimmed.contains(" wrote:") {
            // Attribution line before quotes
            out.push_str(&format!("> {}\r\n", trimmed));
        } else {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }

    out
}

// Formats patch/diff content with proper code blocks and ANSI colors
fn format_patch_body(body: &str) -> String {
    let mut out = String::new();
    let mut in_diff_block = false;
    let mut current_file: Option<String> = None;

    for line in body.lines() {
        // Detect start of a new file diff
        if line.starts_with("diff --git") {
            // Close previous block if open
            if in_diff_block {
                out.push_str("```\r\n\r\n");
            }

            // Extract filename from "diff --git a/foo b/foo"
            if let Some(fname) = line.split(" b/").last() {
                current_file = Some(fname.to_string());
            }

            // Start new diff section with a header
            if let Some(ref fname) = current_file {
                out.push_str(&format!("### {}\r\n", fname));
            }
            out.push_str("```diff\r\n");
            out.push_str(line);
            out.push_str("\r\n");
            in_diff_block = true;
            continue;
        }

        // If we're in a diff block, keep adding lines with ANSI colors
        if in_diff_block {
            // Check if we've left the diff
            let dominated_by_diff = is_diff_line(line) || line.is_empty();
            if !dominated_by_diff && !line.starts_with("--") {
                // End the diff block
                out.push_str("```\r\n\r\n");
                in_diff_block = false;
                out.push_str(line);
                out.push_str("\r\n");
            } else {
                // Apply ANSI colors based on line type
                // Addition lines (+ but not +++)
                if line.starts_with('+') && !line.starts_with("+++") {
                    out.push_str(&format!("{}{}{}\r\n", ANSI_GREEN, line, ANSI_RESET));
                }
                // Deletion lines (- but not ---)
                else if line.starts_with('-') && !line.starts_with("---") {
                    out.push_str(&format!("{}{}{}\r\n", ANSI_RED, line, ANSI_RESET));
                }
                // Hunk headers (@@ ... @@)
                else if line.starts_with("@@") {
                    out.push_str(&format!("{}{}{}\r\n", ANSI_CYAN, line, ANSI_RESET));
                }
                // Everything else (context lines, etc)
                else {
                    out.push_str(line);
                    out.push_str("\r\n");
                }
            }
            continue;
        }

        // Regular line outside diff
        out.push_str(line);
        out.push_str("\r\n");
    }

    // Close any remaining open block
    if in_diff_block {
        out.push_str("```\r\n");
    }

    out
}

// Formats the email body for Gemini output
// For patches: wraps diff sections in preformatted blocks
// For regular emails: formats quoted text properly
fn format_body_for_gemini(body: &str) -> String {
    if looks_like_patch(body) {
        return format_patch_body(body);
    }

    format_quoted_text(body)
}

// Undoes format=flowed (RFC 3676)
//
// format=flowed allows emails to have "soft" line breaks that receivers can reflow
// to fit their display width. The rules are:
//
// 1. Flowed lines: end with trailing space, should be joined with next line
//    Example: "Hello " + "world" -> "Hello world"
// 2. Fixed lines: no trailing space, these are hard breaks (paragraph ends)
// 3. Space-stuffing: lines starting with space have an extra leading space
//    added by sender to avoid parsing issues - we strip it here
//
// Example input (trailing spaces shown as •):
//   "This is a long•"
//   "paragraph.•"
//   "New paragraph."
//
// Output:
//   "This is a long paragraph."
//   "New paragraph."
fn unformat_flowed(text: &str) -> String {
    let mut result = String::new();
    // Tracks whether we just processed a flowed line
    // If true, don't add newline before next line (we're continuing a paragraph)
    let mut skip_newline = true;

    for line in text.split('\n') {
        // Strip space-stuffing: remove leading space added by sender
        let line = line.strip_prefix(' ').unwrap_or(line);

        // Add newline before this line unless we're continuing from a flowed line
        if !skip_newline {
            result.push('\n');
        }

        if let Some(content) = line.strip_suffix(' ') {
            // Flowed line: trailing space means "join with next line"
            result += content;
            result.push(' ');
            skip_newline = true;
        } else {
            // Fixed line: no trailing space means "hard break here"
            result += line;
            skip_newline = false;
        }
    }
    result
}

// Represents the Gemini output for a thread: an index page plus individual message pages
pub struct ThreadPages {
    pub index: String,
    pub messages: Vec<String>,
}

// Generate the main index page listing all mailing lists
fn generate_lists_index(lists: &[MailingList]) -> String {
    let mut out = String::from("# Archives\r\n\r\n");

    for list in lists {
        out.push_str(&format!("=> ./{0}/index.gmi {0}\r\n", h(&list.name)));
    }

    out
}

// Generate index pages for a single mailing list (with pagination)
fn generate_list_index(list: &MailingList, thread_summaries: &[ThreadSummary]) -> Vec<String> {
    let page_count = (thread_summaries.len() / PAGE_SIZE) + 1;

    thread_summaries
        .chunks(PAGE_SIZE)
        .enumerate()
        .map(|(n, topics)| {
            // Header section
            let mut out = format!("# {}\r\n\r\n", list.name);

            // Thread listings
            for (i, topic) in topics.iter().enumerate() {
                if i > 0 {
                    out.push_str("\r\n");
                }

                let path_id = h(&topic.first_message_id.replace('/', ";"));
                let subject = h(&topic.first_message_subject);
                let from = h(topic
                    .first_message_from
                    .name
                    .as_ref()
                    .unwrap_or(&topic.first_message_from.address));

                let date_str = topic.last_reply.to_rfc3339();
                let date = date_str
                    .split_once('T')
                    .map(|(d, _)| d)
                    .unwrap_or(&date_str);
                let replies = topic.reply_count;

                // Thread link with subject
                out.push_str(&format!("=> threads/{}/index.gmi {}\r\n", path_id, subject));

                // Meta line: from, replies, date
                let reply_word = if replies == 1 { "reply" } else { "replies" };
                out.push_str(&format!(
                    "  {} · {} {} · {}\r\n",
                    from, replies, reply_word, date
                ));

                // Preview text using Gemini quote syntax
                if !topic.first_message_preview.is_empty() {
                    out.push_str(&format!("> {}\r\n", h(&topic.first_message_preview)));
                }
            }

            // Pagination
            // Threads are sorted newest first, so lower page = newer, higher page = older
            out.push_str("\r\n───────────────────────────────────────\r\n\r\n");

            if page_count > 1 {
                out.push_str(&format!("Page {} of {}\r\n\r\n", n + 1, page_count));
            }

            if n > 0 {
                if n == 1 {
                    out.push_str("=> index.gmi ← Newer\r\n");
                } else {
                    out.push_str(&format!("=> index-{}.gmi ← Newer\r\n", n));
                }
            }
            if n + 1 < page_count {
                out.push_str(&format!("=> index-{}.gmi Older →\r\n", n + 2));
            }
            out.push_str("=> ../index.gmi All lists\r\n");

            out
        })
        .collect()
}

// Generate thread pages (index + individual messages)
// Thread is sorted newest first, so last element is the original message
fn generate_thread_pages(thread: &[Mail]) -> ThreadPages {
    let msg_count = thread.len();
    let original = &thread[thread.len() - 1];
    let thread_subject = h(&original.subject);

    // Build the index page (table of contents)
    let mut index = format!("# {}\r\n\r\n", thread_subject);
    let msg_word = if msg_count == 1 {
        "message"
    } else {
        "messages"
    };
    index.push_str(&format!("{} {}\r\n\r\n", msg_count, msg_word));
    index.push_str("## Messages\r\n\r\n");

    for (i, msg) in thread.iter().enumerate() {
        let from = h(msg.from.name.as_ref().unwrap_or(&msg.from.address));
        let date_str = msg.date.to_rfc3339();
        let date = date_str.split_once('T').map(|(d, _)| d).unwrap_or("");

        // Link to individual message page
        index.push_str(&format!("=> {}.gmi {} - {}\r\n", i + 1, from, date));

        // Preview
        if !msg.preview.is_empty() {
            index.push_str(&format!("> {}\r\n", h(&msg.preview)));
        }
        index.push_str("\r\n");
    }

    index.push_str("───────────────────────────────────────\r\n\r\n");
    index.push_str("=> ../../index.gmi Back to list\r\n");

    // Build individual message pages
    let messages: Vec<String> = thread
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let mut out = format!("# {}\r\n\r\n", thread_subject);
            out.push_str(&format!("Message {} of {}\r\n\r\n", i + 1, msg_count));

            // Format the sender nicely
            let from_display = msg
                .from
                .name
                .as_ref()
                .map(|n| format!("{} <{}>", n, msg.from.address))
                .unwrap_or_else(|| msg.from.address.clone());

            // Build header block
            out.push_str(&format!("## {}\r\n\r\n", h(&msg.subject)));
            out.push_str(&format!("From: {}\r\n", h(&from_display)));

            let date_str = msg.date.to_rfc3339();
            let date_formatted = date_str
                .split_once('T')
                .map(|(d, t)| format!("{} {}", d, t.split_once('.').map(|(x, _)| x).unwrap_or(t)))
                .unwrap_or_default();
            out.push_str(&format!("Date: {}\r\n", date_formatted));

            // Message-ID
            out.push_str(&format!("Message-ID: {}\r\n", h(&msg.id)));

            // To field
            if !msg.to.is_empty() {
                let to_str = msg
                    .to
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                out.push_str(&format!("To: {}\r\n", h(&to_str)));
            }

            // CC field
            if !msg.cc.is_empty() {
                let cc_str = msg
                    .cc
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                out.push_str(&format!("Cc: {}\r\n", h(&cc_str)));
            }

            // In-Reply-To
            if let Some(irt) = &msg.in_reply_to {
                out.push_str(&format!("In-Reply-To: {}\r\n", h(irt)));
            }

            out.push_str("\r\n");

            // Message body with quote and diff formatting
            let body = match msg.flowed {
                true => unformat_flowed(&msg.body),
                false => msg.body.clone(),
            };
            let formatted_body = format_body_for_gemini(&body);
            out.push_str(&formatted_body);

            // URL extraction for non-patch messages
            if !looks_like_patch(&body) {
                let urls = extract_urls(&body);
                if !urls.is_empty() {
                    out.push_str("\r\n### Links\r\n");
                    for (idx, url) in urls.iter().enumerate() {
                        out.push_str(&format!("=> {} [{}]\r\n", url, idx + 1));
                    }
                }
            }

            // Footer with navigation
            // Messages are sorted newest first, so lower index = newer, higher index = older
            out.push_str("\r\n───────────────────────────────────────\r\n\r\n");
            out.push_str("=> index.gmi Thread index\r\n");
            if i > 0 {
                out.push_str(&format!("=> {}.gmi ← Newer\r\n", i));
            }
            if i + 1 < msg_count {
                out.push_str(&format!("=> {}.gmi Older →\r\n", i + 2));
            }
            out.push_str("=> ../../index.gmi Back to list\r\n");

            out
        })
        .collect();

    ThreadPages { index, messages }
}

pub fn make_gemini_site(lists: &[MailingList], outdir_path: &str) -> anyhow::Result<()> {
    log::info!("Generating Gemini output in '{}'", outdir_path);

    let out_dir = PathBuf::from(outdir_path);

    // Write main index
    let main_index = generate_lists_index(lists);
    std::fs::write(out_dir.join("index.gmi"), main_index)?;

    // Process each list
    for list in lists {
        let list_dir = out_dir.join(&list.name);
        let thread_dir = list_dir.join("threads");

        std::fs::create_dir_all(&thread_dir)?;

        // Build thread summaries
        let summaries = list.build_thread_summaries();

        // Write list index pages
        let list_pages = generate_list_index(list, &summaries);
        for (n, page) in list_pages.iter().enumerate() {
            let index_name = if n == 0 {
                "index.gmi".to_string()
            } else {
                format!("index-{}.gmi", n + 1)
            };
            std::fs::write(list_dir.join(index_name), page)?;
        }

        // Process each thread
        for thread in &list.threads {
            if thread.is_empty() {
                continue;
            }

            // Thread is sorted newest first, so last element is the original
            let original_msg = &thread[thread.len() - 1];
            let thread_id = original_msg.pathescape_msg_id();
            let thread_subdir = thread_dir.join(&thread_id);
            std::fs::create_dir_all(&thread_subdir)?;

            // Generate thread pages
            let pages = generate_thread_pages(thread);

            // Write thread index
            std::fs::write(thread_subdir.join("index.gmi"), &pages.index)?;

            // Write individual message pages
            for (i, page) in pages.messages.iter().enumerate() {
                std::fs::write(thread_subdir.join(format!("{}.gmi", i + 1)), page)?;
            }
        }

        log::info!(
            "Processed list '{}': {} threads, {} messages",
            list.name,
            list.threads.len(),
            list.id_to_index.len()
        );
    }

    Ok(())
}
