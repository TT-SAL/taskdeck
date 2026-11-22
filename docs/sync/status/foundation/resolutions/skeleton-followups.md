# Spec follow-ups found while writing the skeletons

Writing each crate's public surface from its "Public API" section is a compile check of that section. What
did not compile as written, was abbreviated without a source, or contradicted another section is listed
here. One resolver pass after the last skeleton turns the list into exact changes; where the skeleton
already took a reading, the spec is brought to the skeleton, not the other way round, unless stated.

## core.md (from the core skeleton)

1. §20's comments for `planner`, `tasks`, `archive`, `ics`, `overlay`, `weather` and the snapshot builders
   end in "…" or "today's". The items the skeleton carries (from §14.2's table and today's `src/`, without
   UI-only items and file I/O) are in `crates/taskdeck-core/src/`: §20 lists them by name.
2. §14.2's table puts `now_marker` (and `snap`, `clamp_block`, `resolve_on_day`) in `planner`; §20 puts them
   in `time`. §14.2 follows §20.
3. `Intent::Create.kind` is `board::Kind` (§12.3 says only task, event, routine).
4. `weather::build_report` takes the response types today's code keeps private; they are public in the
   skeleton and §20 names them.
5. The snapshot builders' signatures ("today's, with the zone") are written out as the skeleton has them.

## seal.md (from the seal skeleton)

6. §26: `login_message` and `sign_login` name a parameter `gen`, a reserved word in edition 2024. It is
   `generation` (the skeleton has `r#gen`; the skeleton follows the spec here).
7. §26: `open_snapshot_declared`'s `roots: &dyn Fn(u32) -> Option<&EpochRoot>` needs a lifetime: `<'a>` and
   `Option<&'a EpochRoot>`.
8. §26: `pub enum SealError { /* one variant per code of §22 */ }` gives no names: each variant is the
   CamelCase of its code (`seal/frame_format` is `FrameFormat`).
9. §26 and §17.10: `pair::Existing::start(/* … */)` has no written signature (the skeleton: membership, head,
   relay origin, own dev, now, entropy); `pair::New` has no constructor; §17.10 says time is passed with
   each event, but `on` takes none (the skeleton put `now` on the events that need it).
10. §16.2's `compact` row says "its `SnapInfo.blob`"; §26 names the payload type `SnapAnnounce`.
11. §12.2 says the genesis entry has exactly two wraps; §26 must say whether `Genesis.wraps` is
    `[[u8; 96]; 2]` or a `Vec` (the skeleton has a `Vec`, like `EpochRotated`).

## engine.md (from the sync skeleton and the toy state machine)

12. §37 lists `PendingMark` and `NoticeEntry` twice: in `pub use ui::{…}` and as root structs. They live in
    `ui` and are re-exported.
13. §37 abbreviates `FaultSet { /* §31.6 */ }`, `OpSet { /* §31.5 */ }` and `SimStats { … }`, and §31.5 and
    §31.6 give no field names. They are empty structs in the skeleton; §37 says that stage `engine`, which
    owns the simulator, fills them through a spec change.
14. §5.3 (the toy): the `op`, `target`, `base` and `current` of each toy refusal (the toy uses the op's name
    and the command's target for every code); the error of `rebind` for an origin state that is not 8 bytes
    (`engine/toy_format`) and for a minted `n` above 2^40 − 1 (`engine/toy_bad_id`); and the bounds `load`
    enforces on a mark's device number (1 to 8,191) and `n` (at most 2^40 − 1), which the toy does not
    enforce yet.

## core.md (found by `scripts/spec-lint.sh`, which is stricter than the lead's earlier format check)

15. §16: the error code `core/gone` stands twice, at two places of the catalogue with different meanings
    (lines 1201 and 1227 when found). One of the two gets a name of its own, and every place that cites it
    follows.
16. §16: the table "Intent and compile errors" has the columns Code, Status, Message, Raised when, What the
    caller does; README §6 fixes Code, Meaning, Raised when, What the caller does. The table takes the
    README's four columns, with the status and the message inside "Meaning" (as relay.md's "Meaning
    (status)" does).

## bridge.md and device.md (from the device and android skeletons)

17. bridge.md §21 and §12.4: `PageFacade::answer_snapshot` and `PageFacade::today` take `chrono::NaiveDate`,
    but §12.4's list of the crate's dependencies has no `chrono`. `chrono` joins the list (the workspace pin;
    it is already in the lockfile), and the lead adds it to the manifest. The two methods are missing from
    the skeleton until then.
18. bridge.md §21 writes `pub mod reminders` without a feature gate, but it names `taskdeck_core`, which the
    crate has only with `backend-device`: the module is gated on that feature.
19. device.md §25: the section names the key stores and the file layers but writes no `impl`: say that the
    four key stores implement `KeyStore` and that `OsFiles` and `CrashFiles` implement `FileLayer` (the
    skeleton has these), and that the impls of `StateMachine` for `CoreMachine`, of `Sealer` for `SealSealer`
    and of `Backend` for the two backends are stage `device`'s and `android`'s to write.

## seal.md and engine.md (from the fake sealer)

20. seal.md §14.3 defines only rules version 1; the toy's upgrade path needs to know what a later version
    allows. The fake treats later versions as allowing (1, 1) and hash version 1: one of the two specs says so.
21. engine.md §6.3 and §37: `FakeSealer::create(seed, name)` takes no time and the fake has no way to be given
    its supported rules; the fake uses `at_ms = 0`, the list `[1]` and crate-private setters. Say which of
    these, if any, §37 names publicly for the simulator.
22. seal.md: SEAL-FRAME-004 (a frame with `ctr = 0`) names no catalogue code; the fake answers
    `seal/frame_format`.
23. engine.md §6.1: `open_snapshot` takes `&self` although §6.1 and §6.3 have it keep the body's history;
    the fake uses interior mutability. Either the method takes `&mut self` or the text says the sealer keeps
    that history behind shared access.
24. The workspace-wide clippy command of the briefs lints the root package's `build.rs` since
    `fixtures/generator` depends on the root package, and fails on a warning that today's code has always
    had. The command excludes `taskdeck-fixtures`, and the generator is linted alone with `--no-deps`. (Not
    a spec matter: the briefs and the attack workflows carry the new command.)

