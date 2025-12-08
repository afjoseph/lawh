use mail_parser::{Addr, Address, DateTime, MimeHeaders};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub type MessageID = String;

// Epoch constant for fallback dates
pub const EPOCH: DateTime = DateTime {
    year: 1970,
    month: 1,
    day: 1,
    hour: 0,
    minute: 0,
    second: 0,
    tz_before_gmt: false,
    tz_hour: 0,
    tz_minute: 0,
};

#[derive(Debug, Clone)]
pub struct MailAddress {
    pub name: Option<String>,
    pub address: String,
}

impl MailAddress {
    fn from_addr(addr: &Addr) -> Self {
        MailAddress {
            name: addr.name.as_ref().map(|s| s.to_string()),
            address: addr
                .address
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default(),
        }
    }

    fn from_address(addr: &Address) -> Vec<Self> {
        // Address is an enum: List(Vec<Addr>) or Group(Vec<Group>)
        match addr {
            Address::List(addrs) => addrs.iter().map(Self::from_addr).collect(),
            Address::Group(groups) => groups
                .iter()
                .flat_map(|g| g.addresses.iter().map(Self::from_addr))
                .collect(),
        }
    }
}

impl std::fmt::Display for MailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(n) = &self.name {
            write!(f, "\"{}\" ", n)?;
        }
        write!(f, "<{}>", self.address)
    }
}

pub struct Mail {
    // Extracted fields from the parsed message
    pub id: String,
    pub subject: String,
    pub thread_subject: String,
    pub preview: String,
    pub from: MailAddress,
    pub date: DateTime,
    pub body: String,
    pub flowed: bool,
    pub in_reply_to: Option<String>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
}

impl Mail {
    // Path-escaped message ID for use in filenames
    // Replaces / with ; to avoid path issues
    pub fn pathescape_msg_id(&self) -> PathBuf {
        PathBuf::from(self.id.replace('/', ";"))
    }

    fn from_parsed_message(msg: &mail_parser::Message) -> Self {
        let id = msg.message_id().unwrap_or("").to_string();
        let subject = msg.subject().unwrap_or("(No Subject)").to_string();
        let thread_subject = msg.thread_name().unwrap_or("(No Subject)").to_string();

        let preview = msg
            .body_preview(80)
            .map(|b| b.to_string())
            .unwrap_or_default();

        // Extract from address
        let from = msg
            .from()
            .and_then(|addr| {
                let addrs = MailAddress::from_address(addr);
                addrs.into_iter().next()
            })
            .unwrap_or(MailAddress {
                name: None,
                address: "invalid-email".to_string(),
            });

        let date = msg.date().cloned().unwrap_or(EPOCH);

        // Extract to addresses
        let to = msg
            .to()
            .map(|addr| MailAddress::from_address(addr))
            .unwrap_or_default();

        // Extract cc addresses
        let cc = msg
            .cc()
            .map(|addr| MailAddress::from_address(addr))
            .unwrap_or_default();

        let in_reply_to = msg.in_reply_to().as_text().map(|a| a.to_string());

        let body = msg
            .body_text(0)
            .unwrap_or(Cow::Borrowed("[No message body]"))
            .to_string();

        // Check for format=flowed
        let flowed = msg
            .content_type()
            .and_then(|ct| ct.attribute("format"))
            .map(|f| f.eq_ignore_ascii_case("flowed"))
            .unwrap_or(false);

        Mail {
            id,
            subject,
            thread_subject,
            preview,
            from,
            date,
            body,
            flowed,
            in_reply_to,
            to,
            cc,
        }
    }
}

// Summary of a thread for list index display
pub struct ThreadSummary {
    pub first_message_id: String,
    pub first_message_subject: String,
    pub first_message_from: MailAddress,
    pub first_message_preview: String,
    pub reply_count: usize,
    pub last_reply: DateTime,
}

pub struct MailingList {
    pub name: String,
    pub threads: Vec<Vec<Mail>>,
    pub id_to_index: HashMap<MessageID, usize>,
}

impl MailingList {
    pub fn from_maildir_path(maildir_path: &std::path::PathBuf) -> anyhow::Result<Self> {
        let iter_new = Self::parse_maildir_subdir(&maildir_path.join("new"))?;
        let iter_cur = Self::parse_maildir_subdir(&maildir_path.join("cur"))?;
        let mut all_entries = iter_new;
        all_entries.extend(iter_cur);

        let mut threads: Vec<Vec<Mail>> = Vec::new();
        // Maps message ID to thread index
        let mut id_to_index: HashMap<MessageID, usize> = HashMap::new();
        // Maps thread subject to thread index
        let mut subject_to_index: HashMap<String, usize> = HashMap::new();

        // Consume all_entries and move each Mail into the appropriate thread
        for m in all_entries.into_iter() {
            if id_to_index.contains_key(&m.id) {
                log::warn!("Duplicate message ID found: {}. Skipping.", m.id);
                continue;
            }

            log::info!("Mail ID: {}, Thread Name: {}", m.id, m.thread_subject);

            // If thread exists, append to it
            // Otherwise create new thread
            let mail_id = m.id.clone();
            let thread_name = m.thread_subject.clone();
            let idx = if let Some(&thread_idx) = subject_to_index.get(&thread_name) {
                threads[thread_idx].push(m);
                thread_idx
            } else {
                threads.push(vec![m]);
                threads.len() - 1
            };
            id_to_index.insert(mail_id, idx);
            subject_to_index.insert(thread_name, idx);
        }

        // Sort mails in each thread by date, newest first
        for thread in threads.iter_mut() {
            thread.sort_by(|a, b| b.date.to_timestamp().cmp(&a.date.to_timestamp()));
        }

        Ok(MailingList {
            name: maildir_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            threads,
            id_to_index,
        })
    }

    // Build thread summaries sorted by last reply date (newest first)
    pub fn build_thread_summaries(&self) -> Vec<ThreadSummary> {
        let mut summaries: Vec<ThreadSummary> = self
            .threads
            .iter()
            .filter(|thread| !thread.is_empty())
            .map(|thread| {
                // Thread is sorted newest first, so last element is the original message
                let original = &thread[thread.len() - 1];
                let newest = &thread[0];

                ThreadSummary {
                    first_message_id: original.id.clone(),
                    first_message_subject: original.subject.clone(),
                    first_message_from: original.from.clone(),
                    first_message_preview: original.preview.clone(),
                    reply_count: thread.len() - 1,
                    last_reply: newest.date.clone(),
                }
            })
            .collect();

        // Sort by last reply date, newest first
        summaries.sort_by(|a, b| {
            b.last_reply
                .to_timestamp()
                .cmp(&a.last_reply.to_timestamp())
        });

        summaries
    }

    fn parse_maildir_subdir(subdir_path: &std::path::PathBuf) -> anyhow::Result<Vec<Mail>> {
        let mut entries: Vec<Mail> = Vec::new();

        for subdir_entry in std::fs::read_dir(subdir_path)? {
            let subdir_entry = subdir_entry?;
            let file_name = subdir_entry.file_name().into_string().unwrap();
            if file_name.starts_with('.') {
                continue;
            }

            let data = fs::read(&subdir_entry.path())?;
            let parsed = mail_parser::MessageParser::default()
                .parse(&data)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse mail: {}", file_name))?;

            let mail = Mail::from_parsed_message(&parsed);
            entries.push(mail);
        }
        Ok(entries)
    }
}
