// Family: the typical day and the import (relay.md §16.2). The two cases are
// scripts: day.test.mjs replays them from these counts and checks the totals.
const LIMITS = { worker_requests: 100000, object_requests: 100000, rows_written: 100000, rows_read: 5000000 };
export const day = [
  {
    name: 'the typical day: three devices, 300 edits, 50 phone opens, 48 background syncs',
    clauses: ['RELAY-ROWS-002'], targets: ['core', 'worker'], script: 'day',
    counts: { desktop_connections: 20, phone_opens: 50, socket_appends: 283, catch_up_entries: 6, background_syncs: 48, background_appends: 20, snapshot_parts: 3, caps_and_retries: 10, compaction_deletes: 305 },
    limits: LIMITS, share: 0.02,
  },
  {
    name: 'a 5,000-command import over two UTC days',
    clauses: ['RELAY-ROWS-003'], targets: ['core', 'worker'], script: 'import',
    counts: { frames: 5000, per_append: 16, first_day: 3000 },
    expect: { worker_requests: 317, object_requests: 317, rows_written: 10319 },
    limits: LIMITS, share: 0.2,
  },
];
