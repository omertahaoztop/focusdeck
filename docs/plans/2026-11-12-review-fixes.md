# focusdeck review fixes: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every finding of the 2026-11-12 review is fixed on one branch and lands as one pull request: no note is lost on paste, on a failed drop or on a killed capture run; the board is usable without colour and without a mouse; plugins and registries are held to bounded input and HTTPS; the tests that could not fail now can.

**Architecture:** Six tasks with disjoint files. Task 1 adds two shared pieces (`crate::private` for owner-only files, a fallible `Paths::from_env`). Tasks 2, 3 and 4 run in parallel on top of it: UI behaviour, host/install hardening, capture/queue/remind. Task 5 (rendering) builds on Task 2's `App` API. Task 6 (CI and docs) comes last because the README describes the final behaviour.

**Tech Stack:** Rust 2024 (rustc ≥ 1.89, std `File::lock` / `File::try_lock`), tokio (signal, process), crossterm 0.29 (bracketed paste), ratatui 0.30, reqwest (rustls), `unicode-segmentation` 1 (new direct dependency, already in `Cargo.lock`), cargo-deny in CI.

**Spec:** `docs/superpowers/specs/2026-11-12-review-fixes-design.md` — finding IDs (U1, S4, …) below refer to its tables.

## Global Constraints

- Repo: `/home/omertahaoztop/Desktop/personal/focusdeck`, branch `review-fixes`.
- `focusdeck-protocol` types are untouched; protocol version stays 1. The plugins repository is untouched.
- The only new dependency is `unicode-segmentation` (Task 2). No `#[allow]`, no `#[ignore]`, no stubs.
- A task edits only the files its **Files** list names. A file listed with a restriction is edited only inside that restriction.
- New user-facing text goes into both `crates/focusdeck/locales/en.ftl` and `tr.ftl`, directly under the existing keys with the same prefix (parity test). Turkish uses the real letters ç ğ ı İ ö ş ü. Log lines stay plain English, as today.
- Tests do not call `i18n::init`; library tests expect English. Tests that run the binary pin `LC_ALL`. No real person, company or work data in tests.
- Comments say why, in one line. No banners. At most one header line per file.
- Commits: English, imperative, one line, at most 72 characters, no body, no prefix.
- After every task: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all --check` pass.
- Reviewer notes with proposed code for many findings are in the SDD workspace for the duration of the run: `ui-review.md` and `testing-review.md`. They are proposals; this plan and the spec win where they differ.

## Review Focus

1. A capture run killed at any point loses no note; two runs at once deliver no note twice. (Task 4)
2. A failed reinstall leaves the previous plugin byte-identical. (Task 3)
3. No plugin output, however long, grows host memory past the cap. (Task 3)
4. `http://` to a non-loopback host is refused for the index and for the download, redirects included. (Task 3)
5. A refresh that arrives mid-drag or mid-selection never retargets the action to another item. (Task 2)
6. Text pasted into the board never runs as commands. (Task 2)
7. With `NO_COLOR=1` no SGR colour sequence is emitted and the selected row is still reverse video. (Task 5)

---

### Task 1: Shared helpers: owner-only files and fallible paths

**Files:**
- Create: `crates/focusdeck/src/private.rs`
- Modify: `crates/focusdeck/src/lib.rs` (`pub mod private;`), `crates/focusdeck/src/paths.rs`, `crates/focusdeck/src/main.rs`, `crates/focusdeck/src/local/plugin.rs` (`serve_stdio` only), `crates/focusdeck/src/notify.rs` (delete the unused `send`)
- Modify: `crates/focusdeck/locales/en.ftl`, `crates/focusdeck/locales/tr.ftl`

**Interface (later tasks rely on it):**

```rust
// private.rs
pub const LOG_LIMIT: u64 = 1024 * 1024;
/// Opens for append with mode 0600, creating the parent directory; a wider existing mode is tightened.
pub fn append(path: &Path) -> io::Result<File>;
/// `append`, after moving a log that passed LOG_LIMIT to "<path>.1".
pub fn log(path: &Path) -> io::Result<File>;
/// Creates or truncates with mode 0600 and writes all bytes.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()>;

// paths.rs
impl Paths { pub fn from_env() -> Result<Paths, String>; }
```

Message key: `error-no-base-dir = { $var } is not set and no home directory was found`.

- [ ] **Step 1:** Tests in `private.rs`: new file is 0600; an existing 0644 file becomes 0600; `log` rotates a file over the limit and keeps the old bytes in `.1`; `write` replaces content and is 0600.
- [ ] **Step 2:** Implement `private.rs`.
- [ ] **Step 3:** Tests in `paths.rs`: an override for all three wins without a home; a missing base directory with no override is `Err` naming the variable. `from_lookup` takes the three defaults as arguments so the test needs no environment.
- [ ] **Step 4:** `Paths::from_env` returns `Result`; `main.rs` reports the error and exits 1; `init_language` falls back to no config; `serve_stdio` maps it to `io::Error`. Delete `notify::send`.
- [ ] **Step 5:** Full suite, clippy, fmt. Commit: `Add owner-only file helpers and fail without a base directory`.

---

### Task 2: UI behaviour

**Review:** per-task — input handling (paste), state interleaving (refresh vs drag), external process (xdg-open)

**Files:**
- Modify: `crates/focusdeck/src/ui/app.rs`, `crates/focusdeck/src/ui/run.rs` (everything except the body of `build_host`), `crates/focusdeck/src/links.rs`
- Modify: `Cargo.toml`, `crates/focusdeck/Cargo.toml`, `Cargo.lock` (`unicode-segmentation = "1"` as a workspace dependency)
- Modify: `crates/focusdeck/locales/en.ftl`, `tr.ftl` — `flash-*` keys only
- Touch `ui/render.rs`, `ui/hint.rs`, `ui/layout.rs` only as far as a renamed or removed `App` item forces a compile fix; visual changes belong to Task 5.

**Interface (Task 5 relies on it):**

```rust
pub struct BoxState { /* existing fields */ pub pending: usize }   // commands in flight, refreshes included
impl App {
    pub fn on_paste(&mut self, text: &str) -> Vec<Effect>;
    /// The search text that narrows box `idx` right now: the live query while searching, the kept filter after Enter.
    pub fn active_filter(&self, idx: usize) -> Option<&str>;        // None when empty
    pub pending_capture: Option<(usize, String)>,                    // field: target box and text of a capture in flight
    pub confirm: Option<(usize, String)>,                            // field: box and item id armed for deletion
}
pub enum Effect { Refresh(usize), /* others unchanged */ }
pub fn fold(s: &str) -> String;                                      // language-independent
```

**Behaviour (spec IDs):**

1. **U1 paste.** `enter_terminal` also enables bracketed paste and the guard disables it. `Event::Paste(text)` goes to `App::on_paste`. In capture and search mode the text is appended with every run of `\r`/`\n` replaced by one space and other control characters dropped; in every other mode nothing happens. Test: `paste_in_capture_keeps_all_lines_as_text`, `paste_on_the_board_runs_nothing` (items unchanged, no effects, mode unchanged).
2. **U2 capture kept.** `Runtime::send` returns `bool` (queued or not). `Runtime::capture`: when the drop was queued, `app.pending_capture = Some((target, text))`; when not, the capture field reopens with the text (the flash from `send` stays). `on_event` `Finished`: a failed `drop` for the pending target reopens the capture field with the text; a successful one clears it. A parse error also keeps the text in the field. Tests: `failed_drop_reopens_capture_with_the_text`, `busy_box_keeps_the_capture_text`.
3. **U3 confirm.** Spec decision 1. Keys: `flash-confirm-unplan = space again deletes "{ $title }" from { $box }`. Any key other than space clears `confirm`; so does an event that removes the item. Tests: `first_space_on_a_planned_item_only_asks`, `second_space_deletes`, `another_key_cancels_the_delete`.
4. **U4 identity.** `Listed` keeps the selection on the same item id (falls back to the clamped row when the id is gone). `Pressed` stores the item id, not a row; release resolves the id in the current list and does nothing when it is gone. Tests: `refresh_keeps_the_selection_on_the_same_item`, `refresh_during_drag_drops_the_item_that_was_grabbed` (T3).
5. **U5 reorder position.** The `reorder` action's `position` is the index, in the box's unfiltered item list, of the row the item was dropped on. Test: `reorder_in_a_filtered_list_sends_the_full_list_position`.
6. **U6 keyboard reorder.** `Shift+↑` / `Shift+↓` in a zoomed box send the same `reorder` action a one-row drag would, under the same condition the drag uses; at the list edge nothing is sent. Selection follows the item (by id, through U4). Test: `shift_arrows_reorder_the_selected_item`.
7. **U7 navigation.** In a zoomed box `PageUp`/`PageDown` move by the number of rows the box showed in the last frame (10 when unknown), `Home`/`End` jump to the ends. `BackTab` focuses the previous box wherever `Tab` focuses the next. Tests: `page_keys_move_by_a_page`, `home_and_end_jump_to_the_ends`, `backtab_goes_to_the_previous_box`.
8. **U8.** `m` with no box that accepts drops sets `flash-no-move-target = no box here accepts moved items` and opens nothing. Test.
9. **U9.** `q` and `?` work when the profile has no boxes. Test: `empty_profile_still_quits_and_shows_help`.
10. **U10 fold.** `fold` loses its language flag: `İ`, `I`, `ı`, `i` fold to `i`, combining dot above (U+0307) is dropped, everything else `to_lowercase`. Tests: `fold("IŞIK") == fold("ışık")`, `fold("İstanbul") == fold("istanbul")`, a search for `ISSUE` finds `issue` and one for `ısı` finds `ISI`.
11. **U11 pending.** The runtime adds one to `boxes[idx].pending` for every command it queued (refresh included); `on_event` subtracts one in `Listed` and `Finished` (saturating). `toggle_plan` sends nothing while the plan box has `pending > 0`. Test: `double_space_plans_once`.
12. **U12.** `flash-action-failed` and the drop failure flash carry the box title: `{ $box }: { $what } failed: { $message }`. Update both locales and the tests that pin the old text.
13. **U13.** The event loop draws only when something changed. A mouse `Moved` event that changes neither hover nor drag state must not redraw; make that decision a function with a test (`moving_inside_the_same_cell_needs_no_redraw`). Clock ticks still redraw.
14. **U14.** `openable(url) -> bool`: true for `https://`, `http://`, `mailto:` (ASCII case-insensitive scheme), false otherwise. `open_url` refuses the rest with `flash-link-unsupported = cannot open this kind of link: { $url }`. Test: table including `file:///etc/passwd`, `javascript:alert(1)`, `-oProxyCommand=x`, ` https://a`.
15. **U15.** Backspace in capture and search removes the last grapheme cluster (`unicode_segmentation::UnicodeSegmentation::graphemes(true)`). Test with `e\u{301}` and a flag emoji.
16. **U16.** `items_to_close` callers close linked items only after two consecutive successful, empty `Full` lists from that box; any non-empty list or error resets the count. Keep the rule in one place with a test: `one_empty_list_closes_nothing`, `two_empty_lists_close_the_linked_items`.
17. **U17.** `Effect::Refresh(usize)`; drop the view assertions that the runtime ignored.
18. **T6.** `effects_reach_the_box_channel_or_flash` in `run.rs`: a `Runtime` with one closed slot, one full channel, one dropped receiver and one healthy channel; assert the flash text for the first three and the received `BoxCmd` for the fourth, plus the `bool` returned by `send`.
19. Tighten the scroll test that asserts a range into the exact offset (T13).

- [ ] **Step 1:** Read `ui-review.md` findings for the items above and the code they point at.
- [ ] **Step 2:** For each numbered behaviour: write the failing test, run it, implement, run it. Keep commits small (one behaviour or a few related ones per commit).
- [ ] **Step 3:** Full suite, clippy, fmt.

---

### Task 3: Host, worker, install and config hardening

**Review:** per-task — security (transport, integrity, resource limits), install rollback

**Files:**
- Modify: `crates/focusdeck/src/host.rs`, `crates/focusdeck/src/worker.rs`, `crates/focusdeck/src/install.rs`, `crates/focusdeck/src/config.rs`, `crates/focusdeck/src/cli.rs`
- Modify: `crates/focusdeck/src/ui/run.rs` — the body of `build_host` only
- Modify: `crates/focusdeck/tests/host.rs`, `crates/focusdeck/tests/install.rs`
- Modify: `crates/fake-plugin/src/main.rs` — `main` and new free functions below it only (raw protocol misbehaviour that must bypass `serve`)
- Modify: `crates/focusdeck/locales/en.ftl`, `tr.ftl` — `host-*` and `install-*` keys only

**Interface:**

```rust
// host.rs
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_LINE: usize = 64 * 1024;
// install.rs
pub fn verify_installed(plugin_dir: &Path, name: &str) -> anyhow::Result<()>;  // Ok when no manifest exists
```

**Behaviour (spec IDs):**

1. **S1 line cap.** The host reads replies with a capped line reader (fill/consume on the `BufReader`, never `read_line`/`lines`). A reply over `MAX_LINE_BYTES` returns `HostError::BadLine(host-line-too-long)` and the child is killed so the next call starts a fresh one. `host-line-too-long = plugin sent a line longer than 16 MiB`. Stderr lines are cut at `MAX_STDERR_LINE`; the rest of an over-long line is drained without being stored. The log file is opened through `crate::private::log`.
2. **S9 one handshake per command.** In `worker.rs`, a command whose describe fails is answered with that error (`Listed { result: Err }` or `Finished { result: Err }`) without a second spawn or handshake for the same command. A later command tries again; when it succeeds the box is announced (`Described`) and the command runs.
3. **S2 HTTPS.** `fetch` accepts `https://`, `file://` (absolute) and `http://` only when the host is `127.0.0.1`, `::1` or `localhost`. Otherwise `install-insecure-url = refusing plain http (use https): { $url }`. The redirect policy refuses a redirect to non-loopback `http://` whatever the previous hops were.
4. **S3.** In `install`, a download URL starting with `file://` is refused unless the registry URL starts with `file://`: `install-remote-file-target = the index at { $registry } points { $name } at a local file`.
5. **S4 rollback.** A reinstall whose manifest (or binary) write fails leaves the previous binary and manifest byte-identical; a failed first install leaves neither file and no `.part`/backup file behind. Stage everything fallible before the first rename; keep the previous binary reachable (hard link or rename aside) until the manifest is in place.
6. **S8.** `verify_installed` compares the SHA-256 of `plugin_dir/name` with `sha256` in `plugin_dir/name.toml`. No manifest: `Ok`. Unreadable or unparsable manifest, or a mismatch: `install-binary-changed = { $name } no longer matches its manifest; run "focusdeck plugin install { $name }" again`. `build_host` calls it for every non-local plugin after the `is_file` check and returns its message as the box error.
7. **S7.** `Config` gets `#[serde(deny_unknown_fields)]`. Test: `registires = [...]` fails and the error names the key.

**Tests (spec IDs T1, T4, T7, T8, T9, T11, T13):**

- `failed_reinstall_keeps_the_previous_plugin` and `failed_first_install_leaves_nothing`: force the manifest step to fail after the binary is staged (for example a non-empty directory where the manifest part file goes) and compare bytes.
- `protocol_violations_are_reported`: table over fake-plugin raw modes: non-UTF-8 line, non-JSON line, wrong id, oversized line (cap lowered through a test-only constructor or builder, not a 16 MiB payload), exit before replying. Assert the exact `HostError` variant and, where there is text, the message.
- `oversized_stderr_line_is_cut_in_the_log`.
- Worker: `failed_describe_reports_once_per_command` (count plugin starts through the fake plugin's log), `describe_recovers_on_a_later_command`.
- Registry precedence: `unreachable_first_registry_falls_through`, `first_registry_wins_and_the_second_is_not_asked`.
- `plain_http_to_a_remote_host_is_refused` (index and download), `loopback_http_is_allowed`, `remote_index_cannot_point_at_a_local_file`, `redirect_to_plain_http_is_refused`.
- `changed_binary_does_not_start`, `plugin_without_manifest_still_starts`.
- Host tests default to a 10 s timeout; 500 ms stays only in tests of the timeout. Delete the half of `host_passes_requested_language` that cannot fail. Replace `contains` assertions on URL errors and install output with the exact strings.

- [ ] **Step 1:** Read `testing-review.md` (rollback, protocol violations, worker, registry, timeouts) and the code.
- [ ] **Step 2:** For each behaviour: failing test, implement, pass. Small commits.
- [ ] **Step 3:** Full suite, clippy, fmt.

---

### Task 4: Capture, queue, remind and local box

**Review:** per-task — concurrency / persisted format (capture queue), data loss

**Files:**
- Modify: `crates/focusdeck/src/capture_cmd.rs`, `crates/focusdeck/src/queue.rs`, `crates/focusdeck/src/capture.rs`, `crates/focusdeck/src/notify.rs`, `crates/focusdeck/src/remind.rs`, `crates/focusdeck/src/local/plugin.rs`, `crates/focusdeck/src/local/store.rs`
- Modify: `crates/focusdeck/tests/commands.rs`
- Modify: `crates/fake-plugin/src/main.rs` — the bodies of `reminders` and `drop_item` only
- Modify: `crates/focusdeck/locales/en.ftl`, `tr.ftl` — `capture-*` and `remind-*` keys only

**Interface:**

```rust
// queue.rs — take_all and put_back are removed; every caller and test moves to these
pub struct Taken { pub items: Vec<Queued>, /* private: the aside files, each held under an exclusive lock */ }
impl Queue {
    pub fn push_entry(&self, entry: &Queued) -> io::Result<()>;
    pub fn take(&self) -> io::Result<Taken>;
    pub fn commit(&self, taken: Taken, unsent: &[Queued]) -> io::Result<()>;
}
```

**Behaviour (spec IDs):**

1. **D1 queue, at least once.**
   - `take`, under the queue lock: every leftover aside file that can be locked with `try_lock` is taken (one that is locked belongs to a live run and is skipped); the live file is renamed aside and locked; all taken files are read. Nothing is deleted.
   - `commit`, under the queue lock: `unsent` is appended to the live file, then the taken aside files are removed.
   - A `Taken` dropped without `commit` leaves its files on disk; the next `take` picks them up.
   - The queue file, its lock file and aside files are created 0600 (`crate::private`).
   - Tests: `in_flight_aside_is_not_taken_twice`, `aside_of_a_dead_run_is_taken_again`, `commit_keeps_unsent_and_removes_asides`, `dropping_taken_loses_nothing`, plus the existing queue tests moved to the new API with their intent kept (ordering of leftovers, truncated lines, read error loses nothing).
2. **D1 capture run.** `capture_cmd::run` writes the new note to the queue first, then takes, delivers in order from a `VecDeque` (D7), then commits with what is left. `SIGTERM` and `SIGINT` (tokio `signal`, raced against delivery with `select!`) stop delivery; the run commits the unsent notes, logs it, sends the "queued" notification with `capture-interrupted = interrupted before the note was sent` and returns 1. The `Unsent` drop guard goes away.
   - Integration tests in `tests/commands.rs`: `sigterm_during_delivery_keeps_every_note` (slow fake plugin, `kill -TERM`, exit code 1, queue holds the note, a second run delivers it exactly once) and `sigkill_during_delivery_loses_nothing` (`kill -KILL`, second run delivers the note).
3. **D2 count.** The "sent from the queue" line counts only queued notes that were delivered. Tests: `unparsable_queued_note_is_not_counted_as_sent`, `partial_flush_requeues_only_the_unsent_notes` (fake plugin `crash-on:<title>` mode in `drop_item`: the process exits when that title arrives; assert the first note is delivered once and the rest are in the queue, in order).
4. **D3, D4 remind.** With `send`, the run holds an exclusive lock on `reminded.json.lock` from before `State::load` until after `save`. Boxes are asked concurrently (`futures_util::future::join_all`); the output order stays sorted by time. `reminded.json` is written through `crate::private::write` (0600) and still replaced atomically. Fake plugin: a mode that fails `reminders`. Test: `remind_logs_a_failing_box_and_still_shows_the_others` replaces the half that ran a healthy plugin (T2); `concurrent_remind_runs_notify_once`.
5. **D5 due time.** `capture.rs`: `fn local_time<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<DateTime<FixedOffset>>` — a single mapping is used, an ambiguous one takes the earliest, a time inside a gap moves to the first valid minute after it. Due times resolve through `chrono::Local` when `now` carries the offset the local zone had at that instant, and through `now`'s fixed offset otherwise (tests, replayed notes from another zone). `parse`'s signature does not change. Test the three branches with a small test-only `TimeZone` that has one gap and one overlap.
6. **D6 positions.** `place_between` reports when the two neighbours are closer than a minimum gap (`1e-6`); the caller then renumbers the whole list in steps of `POSITION_STEP` in its current order and places the item again. Test: 80 reorders into the same slot keep every position distinct and the order right.
7. **T12.** `new_id` takes its candidate source as a parameter internally (`new_id_with`), and the test forces a first candidate that collides.
8. `notify::log_line` opens the log through `crate::private::log`.
9. **T13.** `tests/commands.rs`: the due assertion checks the exact time (`14:30` in the fixed offset), not `is_some()`.

- [ ] **Step 1:** Read `testing-review.md` (remind test, queue flush, weak assertions) and the code.
- [ ] **Step 2:** For each behaviour: failing test, implement, pass. Small commits.
- [ ] **Step 3:** Full suite, clippy, fmt.

---

### Task 5: UI rendering

**Files:**
- Modify: `crates/focusdeck/src/ui/render.rs`, `crates/focusdeck/src/ui/hint.rs`
- Modify: `crates/focusdeck/locales/en.ftl`, `tr.ftl` — `help-*`, `hint-*`, `box-*` keys only

Depends on Task 2 (`BoxState::pending`, `App::active_filter`, the new keys it binds).

**Behaviour (spec IDs):**

1. **R1 NO_COLOR.** One helper decides colour: `fn colour(style: Style, c: Color, on: bool) -> Style`; `on` is false when `NO_COLOR` is set and non-empty, read once. With colour off no `fg`/`bg` is ever set, so the backend emits modifiers only. Test with a `TestBackend`: with colour off, every cell has `Color::Reset` fg and bg, and the selected row's cells carry `Modifier::REVERSED`.
2. **R2.** Overdue rows start with `! ` before the title, colour or not. Test.
3. **R3.** Focused box and every overlay: `BorderType::Double`; hovered drop target keeps `Thick`; others `Plain`. Test on the border symbols.
4. **R4.** Hint line and box titles use the default foreground. Subtitles use `Modifier::DIM` without a colour. No `Color::Gray`, `Color::DarkGray` or `Color::Yellow` remains in `render.rs`.
5. **R5.** Board hint order: help, quit, note, then the rest. Test at width 40: the line contains the help and quit hints.
6. **R6.** Box title shows ` · /query` whenever `app.active_filter(idx)` is `Some`. A filtered box with no rows shows one row: `box-no-matches = no matches for "{ $query }"`. Tests.
7. **R7.** Title ends with ` …` while `pending > 0`. Test.
8. **R8.** In capture and search mode the frame's cursor is placed after the input (`Frame::set_cursor_position`); the `▏` glyph is removed. Test on the frame's cursor position for a short and a scrolled input.
9. **R9.** `truncate` cuts on grapheme clusters and never splits one. Tests: `e\u{301}` and a ZWJ emoji at the cut point.
10. **R10.** Help adds `help-select` (arrows, PageUp/PageDown, Home/End), `help-reorder` (Shift+↑↓), `help-mouse` (click, double click, drag) and says in `help-plan` that the second space deletes. Hints mention reorder in a zoomed box. Both locales.
11. **T10.** `control_characters_in_titles_are_neutralised` asserts the exact rendered text of the row (`kötü?[31m başlık`), so removing `display_safe` fails it.

- [ ] **Step 1:** Read `ui-review.md` (NO_COLOR, overdue, contrast, hint line, filter, cursor, truncate, help) and the code after Task 2.
- [ ] **Step 2:** For each behaviour: failing test, implement, pass. Small commits.
- [ ] **Step 3:** Full suite, clippy, fmt.

---

### Task 6: CI, dependency policy and README

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `.github/dependabot.yml`, `deny.toml`
- Modify: `README.md`, `README.tr.md`

**Behaviour (spec IDs C1–C6):**

1. `ci.yml`: `on: push: branches: [main]` plus `pull_request`. `actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0` in every job. The test job adds `cargo fmt --all --check` before the tests. A `deny` job runs `EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2.1.1` (`command: check`). An `msrv` job on `ubuntu-24.04` runs `rustup toolchain install 1.89 --profile minimal` and `cargo +1.89 check --workspace --all-targets --locked`. `permissions: contents: read` stays.
2. `deny.toml`: advisories on (default database), `[bans] multiple-versions = "allow"`, `[sources]` crates.io only, `[licenses] allow` listing exactly the licences the tree needs. Verify locally with the `cargo-deny` binary the controller names in the dispatch; `cargo deny check` must exit 0.
3. `.github/dependabot.yml`: `cargo` and `github-actions`, weekly, `open-pull-requests-limit: 5`.
4. README (English and Turkish, same structure): install line `--tag v0.3.0`; keys table gains paste, `Shift+↑/↓`, `PageUp/PageDown/Home/End`, `Shift+Tab`, the confirm step on space; a short "Security and privacy" note: HTTPS-only registries (loopback HTTP for local testing), manifest check before a plugin starts, files created 0600, log rotation at 1 MiB, unknown `config.toml` keys are errors; `NO_COLOR` is honoured; the capture queue delivers at least once. Only what the code does; check each statement against the merged code.

- [ ] **Step 1:** CI and policy files; run `cargo deny check` locally.
- [ ] **Step 2:** READMEs.
- [ ] **Step 3:** Commits: `Pin CI actions and add format, deny and MSRV checks`, `Add Dependabot for cargo and actions`, `Document the new keys and the security rules`.
