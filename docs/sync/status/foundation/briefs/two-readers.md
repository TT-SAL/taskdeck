# Brief: the two-readers test (stage foundation, task 3d)

SYNC_PLAN.md §6.1: "two agents, given only the specs, each write out the message sequence and the byte layout
(field order, widths, context strings, with cryptographic outputs left symbolic) for six scenarios: first edit,
offline edit replayed, pairing, revocation with rotation, relay rollback, snapshot bootstrap. Any difference is
an ambiguity to fix. Stop after two rounds with no difference, or after four rounds, recording what remains in
`CHANGES.md`."

How it is run here: the six scenarios are three groups of two. In each round two readers per group, A and B,
work alone; a comparer per group lists every difference and writes the exact spec change for each; one fixer
per spec applies the changes. Readers of a later round are new agents.

| Group | Scenarios |
|---|---|
| `edit` | 1 first edit; 2 offline edit replayed |
| `join` | 3 pairing; 6 snapshot bootstrap |
| `trust` | 4 revocation with rotation; 5 relay rollback |

## A. Reader

You are given only the specs: the files under `docs/sync/spec/` (`README.md`, `seal.md`, `core.md`, `relay.md`,
`engine.md`, `device.md`, `bridge.md`, `wordlist.txt`). Open no other file of the repository: not SYNC.md, not
the plan, not the status files, not source code. You do not see the other reader's work.

For each of your two scenarios, write what happens, step by step, as an implementer who has nothing but the
specs would build it. Start from the world below and the scenario's own facts. Where the specs leave you a
choice or say nothing, do NOT pick silently: write the step with the reading you find most natural and add a
line `AMBIGUOUS: <what the spec leaves open, with the section or clause>`.

Format, the same for both readers so the two can be compared:

- One numbered list of steps per scenario, in the order things happen. Each step names the actor (`Desk`,
  `Mac`, `Phone`, `New`, `Relay`, `User`) and one action: a computation, a record made durable, a message
  sent, a check, a state change, something shown to the user.
- Every byte string that is sealed, signed, hashed, stored or sent is laid out as a table: field, width in
  bytes, value. Give concrete values wherever the world fixes them (ids, counters, seqs, times, lengths that
  follow from the content). Every JSON body is written out in full, with the field order the spec's encoding
  gives it.
- Cryptographic outputs stay symbolic, in this notation: `H(x)`, `SIG(key; "context"; bytes)`,
  `AEAD(key; nonce; ad; plaintext)`, `HPKE(recipient; info; aad; plaintext)`, `HKDF(...)`, `MAC(key; bytes)`.
  Name each random value where it is drawn, in draw order: `RAND1[16]`, `RAND2[24]`, and so on, and say what
  it is for. Write context strings in full.
- Cite the clause id or section for every step and every layout.
- After the last step: the state each device and the relay hold (head seq, chain value in symbolic form,
  counters, epoch, what the outbox and the durable files contain), and what the user saw.

Be exact rather than long: no introductions, no restating of the specs.

## B. Comparer

You get the two readers' files for one group. Read both in full, align their steps, and list every difference:
a step one has and the other lacks, a different order of steps or of durable writes, a field in a different
place or with a different width, type or value, a different context string, key, input to a hash or
signature, error code, route or message name, a different outcome. Also list every `AMBIGUOUS:` line either
reader wrote, and every place where both agree with each other but you can show from the spec text that both
are wrong.

For each difference open the specs at the places the readers cite and decide:

- `spec`: the specs are ambiguous, silent or contradictory there. Write the exact change that removes the
  doubt (the resolver's rules: the section, the full new text, clause ids continuing after the highest in the
  topic, every table row it touches; one owner per rule).
- `reader`: the spec is clear and one reader misread it. Cite the text. No change, unless the wording invited
  the misreading; then write the clarifying change.
- `format`: the two say the same thing in different words. No change.

Only `spec` differences and `reader` differences that needed a clarifying change count as differences for the
stop rule.

## The world (all scenarios start here unless a scenario says otherwise)

- **Board.** `board_id` = `BID` (16 bytes), salt = `SALT`. Created by Desk. Rules version 1, epoch 1 (no
  rotation has happened), home generation 1, no purge.
- **Recovery identity.** Signing key `RSIGN`, HPKE key `RHPKE`; its id as the specs derive it; its counter is 1
  (it authored only the first entry).
- **Devices**, all current: Desk (`dev` = `DESK`, device number 1, the first device), Mac (`MAC`, number 2),
  Phone (`PHONE`, number 3). Their signing and HPKE public keys are `DESK_SIGN`, `DESK_HPKE`, and so on. Desk
  and Mac are desktops; Phone is the Android phone.
- **Log.** Seqs 1 to 200 are sequenced and every device has applied all of them. Chain value at 200: `C200`.
  Seq 1 is the first entry. Counters: Desk has authored 121 frames, Mac 60, Phone 18 (and the recovery
  identity 1).
- **Snapshots.** The newest one is at seq 192, written by Desk, blob id `SNAP192`, acknowledged by Mac.
- **Relay.** The Cloudflare relay (it pushes over sockets), host `relay.example`. Each device holds a valid
  device token of generation 1. Desk and Mac are connected with a socket and idle; Phone's app is closed.
- **Board content.** Among others, a task "Call plumber" with item id 1 × 2^40 + 7, created by Desk, no due
  date, not planned, not done. Desk has minted item ids up to n = 40; Mac up to n = 9; Phone up to n = 5.
  No block ids matter here.
- **Clocks and zones.** Every clock is right. `T0` = 2026-11-22T10:00:00.000Z = 1,795,341,600,000 ms. All
  three devices are in the zone Europe/Helsinki.
- **Outboxes** are empty.

## The scenarios

1. **First edit.** At `T0` the user on Desk quick-adds a task named "Pay rent" with no due date. Write
   everything from the user's action until Desk has seen its own entry come back sequenced and Mac has applied
   it: what Desk compiles, checks and makes durable and in which order; the frame, laid out down to the
   plaintext; what travels on Desk's socket in both directions; what the relay checks and writes; what the
   relay sends to Mac; every check Mac makes, in order; what Mac makes durable and applies; chain values.

2. **Offline edit replayed.** Phone has been closed since seq 200. On Phone, at `T0 − 2 h`, the user marked
   "Call plumber" done; at `T0 − 1 h` the user added a task "Buy stamps". Meanwhile two entries were sequenced:
   seq 201 by Desk at `T0 − 90 min` (a new task "Pay rent") and seq 202 by Mac at `T0 − 30 min` (renaming "Call
   plumber" to "Call the plumber"). At `T0` the user opens the Phone app with a working network. Write: what
   Phone made durable at each of its two edits; the start of its session with the relay, message by message;
   how it gets and checks seqs 201 and 202; what its screen shows before and after; how its two entries are
   sealed and sent (counters, batching), what the relay answers, the seqs they get; what happens to "done"
   on the renamed task, on every device; what notices or refusals are recorded and where.

3. **Pairing.** At `T0` the user starts adding a new device on Desk: a second phone, called "Tab", a fresh
   install with no data (`dev` = `TAB`, keys `TAB_SIGN`, `TAB_HPKE`). Write everything until Tab holds the
   board, is a current member and has sent its first entry (the user adds a task "Hello" on Tab): what the
   user does on each device; the ticket; every pairing message with its layout and its MAC; the short
   authentication string; what Desk uploads and appends, and the relay's part; Tab's login; how Tab gets the
   board and what it checks; what Mac shows.

4. **Revocation with rotation.** At `T0` the user on Desk revokes Phone (lost). Phone is offline. Mac is
   online; at `T0 − 5 s` the user on Mac made an edit (renaming "Call plumber" to "Call the plumber") whose
   frame, sealed while Mac's head was 200, reaches the relay just after Desk's revocation entry was sequenced
   at 201. At `T0 + 1 h` Phone comes online and tries to continue, with one queued edit made at `T0 − 1 h`.
   Write: what the user does on Desk; every request Desk sends to the relay and what the relay changes; the
   revocation entry laid out down to its items, wraps and commitment; what Mac does when it receives it; what
   happens to Mac's own frame, step by step; what Phone meets when it returns and what it shows.

5. **Relay rollback.** The relay has been restored from an older copy: it holds seqs 1 to 190 and has
   forgotten 191 to 200 (of those ten, Desk wrote six, Mac three, Phone one). The devices hold everything to
   200. At `T0` Desk's socket reconnects; at `T0 + 30 s` the user on Desk adds a task "Pay rent"; at
   `T0 + 60 s` Mac reconnects. Phone stays offline. Write: the start of Desk's session, message by message;
   how Desk notices, what it records as evidence and what it shows; what happens to the new edit; how the
   missing entries get back onto the relay (who does it, each request and each relay check); Mac's session;
   the state at the end.

6. **Snapshot bootstrap.** A different history: Mac was last connected 400 days before `T0`, when the head
   was seq 100, and it has one edit queued from that time (renaming an existing task). Since then the relay
   has compacted the log: it no longer holds entries below seq 129; the snapshot at seq 192 (by Desk,
   acknowledged by Phone) is the newest. Membership has not changed in between. At `T0` the user opens the app
   on Mac. Write: Mac's session start; how Mac learns it cannot catch up from 101; what the user is asked;
   how Mac finds and fetches the snapshot, request by request; every check on it, in order; what Mac makes
   durable and in which order; how it gets the entries after the snapshot; what happens to the queued edit;
   what Mac tells the user about continuity.
