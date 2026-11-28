//! Every transcript under `fixtures/transcripts/` (core.md §13.5, §18.4), embedded:
//! the zone directory, the transcript's name and its JSON Lines text.

/// `(zone directory, transcript, text)` of each recorded transcript.
pub const FILES: [(&str, &str, &str); 20] = [
    ("america-new-york", "dst-edges", include_str!("../../../../fixtures/transcripts/america-new-york/dst-edges.jsonl")),
    ("america-new-york", "every-command", include_str!("../../../../fixtures/transcripts/america-new-york/every-command.jsonl")),
    ("america-new-york", "id-limits", include_str!("../../../../fixtures/transcripts/america-new-york/id-limits.jsonl")),
    ("america-new-york", "refusals", include_str!("../../../../fixtures/transcripts/america-new-york/refusals.jsonl")),
    ("america-new-york", "seeded-01", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-01.jsonl")),
    ("america-new-york", "seeded-02", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-02.jsonl")),
    ("america-new-york", "seeded-03", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-03.jsonl")),
    ("america-new-york", "seeded-04", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-04.jsonl")),
    ("america-new-york", "seeded-05", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-05.jsonl")),
    ("america-new-york", "seeded-06", include_str!("../../../../fixtures/transcripts/america-new-york/seeded-06.jsonl")),
    ("europe-berlin", "dst-edges", include_str!("../../../../fixtures/transcripts/europe-berlin/dst-edges.jsonl")),
    ("europe-berlin", "every-command", include_str!("../../../../fixtures/transcripts/europe-berlin/every-command.jsonl")),
    ("europe-berlin", "id-limits", include_str!("../../../../fixtures/transcripts/europe-berlin/id-limits.jsonl")),
    ("europe-berlin", "refusals", include_str!("../../../../fixtures/transcripts/europe-berlin/refusals.jsonl")),
    ("europe-berlin", "seeded-01", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-01.jsonl")),
    ("europe-berlin", "seeded-02", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-02.jsonl")),
    ("europe-berlin", "seeded-03", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-03.jsonl")),
    ("europe-berlin", "seeded-04", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-04.jsonl")),
    ("europe-berlin", "seeded-05", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-05.jsonl")),
    ("europe-berlin", "seeded-06", include_str!("../../../../fixtures/transcripts/europe-berlin/seeded-06.jsonl")),
];
