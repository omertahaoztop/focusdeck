# focusdeck review fixes: design

Source: the review-board pass over `main` at `51974aa` (2026-11-12). Verdict was "ship with fixes". This document lists every finding that gets fixed, the behaviour after the fix, and what stays out.

Scores before: Security 7, Cybersecurity 6, Performance 7, Accessibility 4, UX 5, Code quality 7, Testing 7.

## Decisions that change existing behaviour

These are deliberate. Each one is called out in the pull request.

1. **Space on a planned item asks before it deletes.** The first space shows a flash naming the item; a second space on the same item deletes it from the plan box. Any other key cancels.
2. **Plain `http://` registries and downloads are refused** unless the host is loopback (`127.0.0.1`, `[::1]`, `localhost`). `https://` and `file://` keep working.
3. **A registry that is not itself `file://` cannot point a download at `file://`.**
4. **Unknown top-level keys in `config.toml` are an error.** A typo such as `registires` used to fall back silently to the default registry.
5. **An installed plugin whose bytes no longer match its manifest does not start.** The check runs only when `<name>.toml` exists next to the binary.
6. **No base directory, no start.** With neither `FOCUSDECK_*_DIR` nor a home directory, focusdeck exits with an error and no longer writes into `./focusdeck`.
7. **Log, capture queue and `reminded.json` are created with mode 0600**, and an existing file with wider permissions is tightened when it is next opened. The log moves to `focusdeck.log.1` once it passes 1 MiB.
8. **A plugin reply line longer than 16 MiB ends that plugin process** with a "line too long" error.
9. **`o` opens only `https://`, `http://` and `mailto:` links.**
10. **The capture queue delivers at least once.** A capture run that is killed mid-delivery can deliver a queued note twice on the next run; it can no longer lose one.

## Findings and fixes

### UI behaviour (`ui/app.rs`, `ui/run.rs`, `links.rs`)

| ID | Finding | After the fix |
|---|---|---|
| U1 | Pasting several lines saves the first as a note and runs the rest as board commands (reproduced: a pasted second line marked a task done and quit). | Bracketed paste is enabled. A paste in capture or search mode is inserted as text with line breaks turned into spaces. A paste in any other mode is ignored. |
| U2 | A captured note is lost when the drop fails or the box is busy. | The text stays in the capture field until the box accepted the command. A failed drop reopens the capture field with the text and a flash that names the reason. |
| U3 | Space on a planned item deletes the task with no warning. | Decision 1. |
| U4 | Selection and drag source are row numbers, so a refresh retargets them. | Selection follows the item id across refreshes. A press remembers the item id; a release acts on that id or does nothing when the item is gone. |
| U5 | Drag-to-reorder sends the row of the filtered view as the position in the full list. | The position sent is the index in the unfiltered list of the row the item was dropped on. |
| U6 | Reordering needs a mouse. | `Shift+↑` / `Shift+↓` in a zoomed box move the selected item one row, when the plugin offers `reorder`. |
| U7 | Long lists and big boards are hard to reach by keyboard. | `PageUp`, `PageDown`, `Home`, `End` move the selection in a zoomed box. `Shift+Tab` goes to the previous box. |
| U8 | `m` opens an empty picker when no box accepts drops. | A flash says there is nowhere to move it; no picker. |
| U9 | A profile with no boxes ignores `q` and `?`. | Both work. |
| U10 | Search folds case by UI language, so mixed Turkish and English titles are missed. | One fold for both languages: `İ`, `I`, `ı`, `i` all compare equal, the rest goes through `to_lowercase`. |
| U11 | No in-progress state, so a double press creates duplicates. | Each box counts commands in flight (`BoxState::pending`), refreshes included. While the plan box has a command in flight, space on a zoomed item sends nothing. |
| U12 | Error flashes do not name the box. | Action and drop failures carry the box title. |
| U13 | Every mouse move redraws the screen. | A mouse `Moved` event that changes no state does not redraw. |
| U14 | `o` hands any plugin-supplied URL to `xdg-open`. | Decision 9, with a flash for a refused link. |
| U15 | Backspace in capture and search deletes a code point. | It deletes one grapheme cluster. |
| U16 | `links::items_to_close` closes linked items after a single empty list. | It needs two consecutive empty lists from the same box, so one failed or racing refresh closes nothing. |
| U17 | `Effect::Refresh` carries a view the runtime ignores. | `Effect::Refresh(usize)`; the runtime derives the view. |

### UI rendering (`ui/render.rs`, `ui/hint.rs`)

| ID | Finding | After the fix |
|---|---|---|
| R1 | With `NO_COLOR` set the selection highlight disappears (reproduced: reverse video is reset before the row text). | No colour is emitted when `NO_COLOR` is set and non-empty; selection, focus and overdue rely on modifiers and text only. |
| R2 | Overdue is red text only. | Overdue rows start with `! `. |
| R3 | Focus is colour only. | The focused box and overlays use a double border; others a plain one. |
| R4 | Gray, dark gray and yellow fall below contrast minimums on common palettes. | Hints and titles use the terminal's default foreground; subtitles use `DIM` only. |
| R5 | The hint line cuts off help and quit on narrow terminals. | `? help` and `q quit` come first, then note, then the rest. |
| R6 | An active filter is invisible after Enter; no matches shows a blank box. | The title shows `/query` while a filter is active; an empty result shows a "no matches" row. |
| R7 | Commands in flight are invisible. | A box with `pending > 0` shows ` …` after its title. |
| R8 | The capture field hides the real cursor. | The terminal cursor sits at the end of the input; the fake `▏` goes away. |
| R9 | `truncate` measures code points. | It cuts on grapheme clusters. |
| R10 | Help leaves out selection, mouse gestures and reordering. | Help lists them. |

### Host, worker, install, config (`host.rs`, `worker.rs`, `install.rs`, `config.rs`, `paths.rs`)

| ID | Finding | After the fix |
|---|---|---|
| S1 | A plugin can make the host buffer an unbounded line (stdout and stderr). | Decision 8. Stderr lines are cut at 64 KiB before they reach the log. |
| S2 | Plain HTTP registries and downloads are accepted. | Decision 2. |
| S3 | A remote index can name a `file://` download. | Decision 3. |
| S4 | A failed reinstall deletes the working plugin. | A failed reinstall leaves the previous binary and manifest byte-identical. A failed first install leaves neither. |
| S5 | Falling back to `./focusdeck` when no base directory is known. | Decision 6. |
| S6 | Log is world-readable and grows without bound. | Decision 7. |
| S7 | Unknown config keys are ignored. | Decision 4. |
| S8 | An installed binary is trusted forever. | Decision 5. |
| S9 | A plugin that fails its handshake costs two timeouts per command. | One handshake attempt per command; its error is the command's result. |

### Capture, queue, remind, local box (`capture_cmd.rs`, `queue.rs`, `remind.rs`, `capture.rs`, `local/`)

| ID | Finding | After the fix |
|---|---|---|
| D1 | `focusdeck capture` killed mid-delivery loses the new note and every queued one (reproduced with SIGTERM). | Decision 10. The new note is written to the queue before delivery starts. Taken queue files stay on disk, locked, until delivery is committed. SIGTERM and SIGINT stop delivery, return the unsent notes to the queue and exit 1. |
| D2 | The "N notes sent from the queue" count includes notes that were skipped as unparsable. | Only delivered notes are counted. |
| D3 | Two `remind --send` runs at once notify twice. | The run holds a lock on `reminded.json.lock` from load to save. |
| D4 | `remind` asks boxes one after another, so one slow plugin delays all. | Boxes are asked concurrently. |
| D5 | A due time in a DST gap or overlap is dropped or wrong. | The earliest valid local time is used; a time inside a gap moves to the first valid minute after it. |
| D6 | Repeated reorders between the same neighbours run out of float precision and positions collide. | When two neighbours are closer than a minimum gap, the list is renumbered in steps of `POSITION_STEP`. |
| D7 | Delivery removes from the front of a `Vec`. | `VecDeque`. |

### Tests

| ID | Finding | After the fix |
|---|---|---|
| T1 | The reinstall rollback test stops before any file is written. | A test fails the manifest write during a reinstall and compares both files with their previous bytes. |
| T2 | The "failing box" half of the remind test runs a healthy plugin. | The fake plugin fails `reminders` for that box; the test asserts the log line and the other box's output. |
| T3 | A refresh during a drag is untested. | `refresh_during_drag_drops_the_item_that_was_grabbed`. |
| T4 | No protocol-violation branch of the host is tested. | A table test: non-UTF-8, non-JSON, wrong id, oversized line, early exit. |
| T5 | Partial queue flush is untested; a skipped note counts as sent. | `partial_flush_requeues_only_the_unsent_notes`, `unparsable_queued_note_is_not_counted_as_sent`. |
| T6 | The TUI runtime glue is untested. | `effects_reach_the_box_channel_or_flash` covers closed, busy, stopped and delivered. |
| T7 | The worker's describe path is untested. | A failing describe yields one error event per command; a later success announces the box. |
| T8 | Registry precedence is tested on the happy path only. | First registry unreachable, second has the plugin; first registry has it, second is never asked. |
| T9 | Host tests that should pass run with a 500 ms timeout. | 10 s by default; 500 ms only where the timeout itself is under test. |
| T10 | The control-character render test cannot fail. | It asserts the exact sanitised text. |
| T11 | Half of `host_passes_requested_language` always passes. | The redundant half is deleted. |
| T12 | `new_id_is_unique_against_existing` cannot fail. | The id source is injectable; the test forces a collision first. |
| T13 | Weak assertions where the value is known. | Exact due time, exact URL error strings, exact install output. |

### CI and docs

| ID | Finding | After the fix |
|---|---|---|
| C1 | `actions/checkout@v4` is a mutable tag. | Pinned to the commit of v4.4.0. |
| C2 | Branch pushes run CI twice when a pull request is open. | `push` runs on `main` only. |
| C3 | No format, advisory, licence or MSRV check. | `cargo fmt --all --check`, `cargo deny check` and a `1.89` build job. |
| C4 | No dependency updates. | Dependabot for cargo and GitHub Actions, weekly. |
| C5 | README installs `--tag v0.2.0`. | `--tag v0.3.0`, the release that pull request 2 prepares. The tag has to exist before this lands. |
| C6 | New keys and rules are undocumented. | README and README.tr describe paste, the new keys, the confirm step, `NO_COLOR`, HTTPS-only registries and file permissions. |

## Not in this change

- **Signed plugin index.** It needs a signing key owned by the maintainer and a change in the plugins repository. The SHA-256 in the index still depends on the index being fetched over HTTPS from the right place.
- **One shared test harness.** `fake_plugin()` exists in two test files; merging them is a refactor with no behaviour behind it.
- **Protocol changes.** Protocol version stays 1 and `focusdeck-protocol` types are untouched.
- **Version bump.** Pull request 2 owns the 0.3.0 bump.

## Shared pieces

- `crate::private`: `append(path)` and `log(path)` return a `std::fs::File` opened for append with mode 0600 (parent directory created, wider permissions tightened); `log` rotates first. `write(path, bytes)` creates or truncates with 0600.
- `Paths::from_env() -> Result<Paths, String>`.
- Message keys are added to `locales/en.ftl` and `locales/tr.ftl` together, next to the existing keys with the same prefix.
