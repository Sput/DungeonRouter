# Known limitations

DungeonRouter is a one-week experimental MVP, not a production rules service.

- Switchyard is pre-alpha and its configuration format may change. The bundled configuration must be revalidated after upgrades.
- Only SRD 5.1 content is bundled. Commercial books, non-SRD character options, and the revised 2024 rules are intentionally absent.
- FTS5 matches terminology rather than meaning. Paraphrased questions can miss relevant passages; vector retrieval has not been added.
- Citation validation proves that an ID was supplied to the model, not that every claim is logically supported by that passage.
- Cost estimates omit Auto classifier tokens because the current response stream does not expose them to DungeonRouter.
- The hard limit is checked before each request. A final request can take estimated spend slightly above the threshold.
- Campaign notes are stored as readable text in local SQLite. There are no accounts, permissions, encryption at rest, or cloud synchronization.
- The app is loopback-only by default and has no authentication or production deployment hardening.
- Evaluation cases are included, but a complete live score requires a running Switchyard instance and incurs OpenAI API charges.
- Conversation history, response persistence, voice input, mobile apps, and encounter generation are outside the MVP.
