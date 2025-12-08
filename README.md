# lawh

Static Gemini site generator for maildir archives

## Usage

```
# Generate test maildir
cargo run --bin gen_test_maildir -- -o /tmp/test-maildir -n 20

# Generate Gemini site
#
# Expected maildir structure:
#
#   /maildir-root
#     /list-name
#       /cur
#       /new
#       /tmp
#
# Output structure:
#
#   /output
#     index.gmi
#     /list-name
#       index.gmi
#       /threads
#         /message-id
#           index.gmi
#           1.gmi
#           2.gmi
#
cargo run --bin cli -- -i /tmp/test-maildir -d /tmp/output
```

## Etymology

- Lawh (لوح) is Arabic for "tablet" or "board"
- In Sufi cosmology, al-Lawh al-Mahfuz (the Preserved Tablet) is the celestial record on which all events are inscribed
- It represents the eternal archive of divine knowledge, containing everything that was and will be
- Fun, ja? :)
