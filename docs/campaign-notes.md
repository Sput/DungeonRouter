# Campaign notes

DungeonRouter can index private campaign notes alongside the bundled SRD. The MVP accepts UTF-8 Markdown (`.md` or `.markdown`) and plain-text (`.txt`) files up to 256 KiB.

## How it works

The browser reads the selected file and sends its title, filename, and text to `POST /api/notes`. The Rust API validates the input, splits Markdown on its heading hierarchy, and stores the resulting passages in the local SQLite database. The original file is not copied into the repository or a separate upload directory.

Rules and campaign notes are searched independently with SQLite FTS5 and then merged by relevance. Every result retains a source kind. Campaign-note passages are labeled `Campaign` in the interface and identified as table-specific context in the model prompt; they are not presented as official SRD rules.

Available endpoints:

- `POST /api/notes` indexes a note from `{ "title", "filename", "content" }`.
- `GET /api/notes` lists indexed notes and chunk counts.
- `GET /api/notes/:id` returns note metadata.
- `DELETE /api/notes/:id` deletes the note and all indexed passages.

The interface requires confirmation before deletion. Deletion removes both the document record and its FTS-indexed chunks.

## Privacy boundary

Notes remain in the application's local SQLite database. When a question retrieves a campaign-note passage, that passage is included in the prompt sent through Switchyard to the selected OpenAI model. Notes that are not retrieved are not sent with that question. Do not upload secrets or information you are unwilling to send to the configured model provider.

This experimental MVP does not provide user accounts, access controls, encryption at rest, or cloud synchronization. Anyone with access to the local application database can read the indexed note text.
