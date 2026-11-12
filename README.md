# focusdeck

**English** · [Türkçe](README.tr.md)

A terminal board for "everything that is waiting on me". Verbal promises, Redmine issues, starred mail and messages each sit in their own box on one screen. Boxes are filled by plugins, which are separate programs. You can drag items between boxes with the mouse, and the bottom line always tells you what each key does right now.

Targets Linux x86_64 and aarch64. The interface is available in English and Turkish, and a new language is one file (see [Translating](#translating)).

![Main screen: six boxes, overdue items in red](docs/screenshots/en/board.png)

| Zoomed box | Move | Key list |
|---|---|---|
| ![Zoomed Redmine box, planned items starred](docs/screenshots/en/zoom.png) | ![Choosing a target box](docs/screenshots/en/move.png) | ![Key list opened with ?](docs/screenshots/en/help.png) |

The data in the screenshots is made up.

## Install

```sh
cargo install --git https://github.com/omertahaoztop/focusdeck --tag v0.2.0 --locked focusdeck
```

Ready-made plugins live in the [focusdeck-plugins](https://github.com/omertahaoztop/focusdeck-plugins) repository and are installed from its index:

```sh
focusdeck plugin install vikunja
focusdeck plugin list
focusdeck plugin remove vikunja
```

Plugins are written to `~/.local/share/focusdeck/plugins/<name>`. `install` checks the sha256 of the downloaded file against the index and deletes the file if it does not match. `remove` warns if a profile still uses the plugin.

> Plugins run with your user's permissions; there is no sandbox. Make sure you trust a plugin's source before installing it.

## Settings

`~/.config/focusdeck/config.toml`, every key optional:

```toml
language = "en"
default_profile = "home"
registries = ["https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml"]
capture_command = ["rofi", "-dmenu", "-p", "Note"]

[capture_words]
tomorrow = ["tmrw"]
minute = ["mins"]
```

- `language`: the interface language, a code such as `en` or `tr` (see [Translating](#translating)). If it is missing, the core reads `LC_ALL`, `LC_MESSAGES` and `LANG`, in that order, and falls back to English. A cron job, systemd timer or window manager shortcut may not carry `LANG`, so set `language` if `capture` or `remind` run from one of them.
- `capture_words`: extra spellings for the words `capture` understands, added to the active language's own. Each key takes a list; the keys are `tomorrow`, `minute`, `hour`, `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`, and an unknown key is an error.
- `default_profile`: the profile used when neither `--profile` nor `FOCUSDECK_PROFILE` is given.
- `registries`: plugin index URLs, in order. If the same name appears in two indexes, the earlier one wins. Add your own index or a fork here.
- `capture_command`: the command run to ask for one line when `capture` is called without text.

## Profile

`~/.config/focusdeck/profiles/<name>.toml`. The profile is chosen from `--profile`, then `FOCUSDECK_PROFILE`, then `default_profile`.

```toml
name = "home"
capture_into = "verbal"
capture_refs = { R = "https://redmine.example.com/issues/{}" }
capture_label_aliases = { tel = "phone", f2f = "face-to-face" }

[[box]]
id = "verbal"
title = "Verbal promises"
plugin = "vikunja"
row = 0
col = 0
refresh = "1m"
config = { url = "https://vikunja.example.com", token_file = "~/.config/focusdeck/secrets/vikunja", project = "Home" }

[[box]]
id = "redmine"
title = "Redmine plan"
plugin = "redmine"
row = 0
col = 1
plan_into = "verbal"
config = { url = "https://redmine.example.com", key_file = "~/.config/focusdeck/secrets/redmine" }
```

- `row` and `col` are the grid position. Optional `width` and `height` stretch a box over several cells.
- The `config` table is passed to the plugin as is. Each plugin's keys are listed in its README.
- Secrets never go in the profile; only a file path (`token_file`, `key_file`) does. A leading `~` is expanded in every key ending in `_file`. If the file is readable by group or others, the box does not open (`chmod 600`).
- `plan_into`: in this box, the space key moves an item into the target box as "add to my plan". When the source plugin stops listing the item (the issue was closed), the linked target item counts as done too. No task is closed when the source `list` call fails.
- `capture_into`, `capture_refs` and `capture_label_aliases` configure the `capture` command below.

## Screen and keys

The main screen shows the profile's boxes in a grid. A box title shows its item count and how many are overdue; overdue items are red. The bottom line shows the keys for the current screen, and `?` opens the full list. Double click or `Enter` zooms a box, `Esc` goes back.

| Key | Main screen | Zoomed box |
|---|---|---|
| `a` | take a note (into the `capture_into` box) | same |
| `x` | mark the selected item done | same |
| `u` | unstar the selected item (Zulip) | same |
| `m` | move: opens the list of target boxes | same |
| `space` | none | add to / remove from plan in a box with `plan_into` |
| `o` | open the link in the browser | same |
| `/` | none | search |
| `Tab` | next box | none |
| `?`, `q`, `Esc` | help, quit, back | help, quit, back |

Mouse:

- The wheel scrolls inside a box.
- A click selects an item, a double click zooms the box.
- Dragging an item onto another box sends `drop` to the target plugin. For example, dropping a Zulip or mail item onto a Vikunja box turns it into a task.
- Dragging within the same box reorders it. The top item has the highest priority.

## Capturing notes from outside

```sh
focusdeck capture "R12345 called #tel yarın 14:00 // details"
focusdeck capture
```

Without text, `capture_command` asks for one line. The note goes to the profile's `capture_into` box. The command exits with 1 if the title is empty or the plugin cannot be reached. A note that could not be delivered waits in `~/.local/state/focusdeck/capture-queue.jsonl` and is sent with the next successful capture. To bind it to a window manager shortcut (for example `F9` in Hyprland), `focusdeck capture` is enough.

Syntax:

- Everything after `//` is the description.
- If the last word of the title is a number with a unit, such as `30min` (minutes) or `2h` (hours), the due date and reminder are that long from now; if it is `HH:MM`, they are today at that time (tomorrow if already past). It can be preceded by the word for tomorrow or a weekday (see the words below).
- The first `@name` is who it is from. `#word` is a label, translated through `capture_label_aliases`. A digits-only `#123` stays plain text.
- A word that starts with a `capture_refs` prefix followed by at least two digits (`R12345`) becomes a link.
- The remaining words are the title.

The words depend on the interface language, are case-insensitive, and `capture_words` adds to them:

| Language | tomorrow | minute | hour | Monday to Sunday |
|---|---|---|---|---|
| `en` | `tomorrow` | `min`, `m` | `h` | `mon`/`monday`, `tue`/`tuesday`, `wed`/`wednesday`, `thu`/`thursday`, `fri`/`friday`, `sat`/`saturday`, `sun`/`sunday` |
| `tr` | `yarın`, `yarin` | `dk` | `s` | `pzt`/`pts`, `sal`, `çar`/`car`, `per`/`prş`/`prs`, `cum`, `cmt`, `paz` |

For example, in English `focusdeck capture "R12345 called #tel tomorrow 14:00"`; in Turkish `focusdeck capture "R12345 arandı #tel yarın 14:00"`.

## Reminders

```sh
focusdeck remind
focusdeck remind --send
```

Asks plugins that support reminders for the reminders of the last 12 hours. `remind` prints the ones that are due; `--send` shows them as notifications through `notify-send -u critical`. Shown reminders are recorded in `~/.local/state/focusdeck/reminded.json` and never shown again. A systemd timer or cron entry can run `focusdeck remind --send` every minute.

## Plugin protocol

A plugin is a program the core starts once per box and keeps running. One JSON message per line: requests on stdin, responses on stdout. stderr goes to the core's log file. Protocol version 1.

```json
{"id": 7, "method": "list", "params": {"view": "full"}}
{"id": 7, "result": {"items": []}}
{"id": 8, "error": {"message": "401 Unauthorized"}}
```

| Method | Parameters | Response |
|---|---|---|
| `describe` | none | `name`, `version`, `protocol`, `actions`, `accepts_drop`, `reminders` |
| `configure` | the profile's `config` table | empty, `error` on bad settings |
| `list` | `view`: `summary` or `full` | `items` |
| `action` | `item`, `action` (`done`, `delete`, `move`, `reorder`, `unstar`), `args` | the updated item or empty |
| `drop` | `item`, `from` | the created or moved item |
| `reminders` | `since` (RFC 3339) | pairs of `at` and `item` |

- Each request times out after 15 seconds. A timeout, a non-JSON line or a crashed process shows up as a red line in the box; the core restarts the plugin on the next refresh, and other boxes are not affected.
- If `protocol` in the `describe` response differs from the version the core knows, the box does not open.
- The core only checks the `*_file` path and its permissions; the plugin reads the secret from the file itself, so the value never passes through the core.
- The core starts each plugin with the requested interface language in the `FOCUSDECK_LANG` environment variable (for example `tr`) and removes the variable when no language is set. A plugin that has no translation for it uses English.
- A plugin can be written in any language, as long as it speaks the protocol.

For plugins written in Rust, the `focusdeck-protocol` crate has the types, the `Plugin` trait and a `serve` helper that runs the line loop:

```toml
[dependencies]
focusdeck-protocol = { git = "https://github.com/omertahaoztop/focusdeck", tag = "v0.2.0" }
```

Implement the `Plugin` trait and call `focusdeck_protocol::serve(plugin, stdin.lock(), stdout)`.

## Translating

A language is one file: add `crates/focusdeck/locales/<code>.ftl` (for example `de.ftl`) in [Fluent](https://projectfluent.org/) syntax and rebuild; the file name is the code `language` and `LANG` match against.

- `en.ftl` is the base. Copy it and translate the values; message ids and `{ $variables }` stay as they are.
- `cargo test` fails with the missing or extra ids and the variables that differ from `en.ftl`, so an incomplete translation cannot slip through.
- The `capture-words-*` entries are space-separated lists of spellings, not sentences: `capture-words-tomorrow`, `capture-words-minute`, `capture-words-hour` and `capture-words-mon` to `capture-words-sun`. Write the spellings people actually type, lowercase or not, and include the unaccented variant if your keyboard layout makes it likely.
- Texts that a plugin prints are translated in the [focusdeck-plugins](https://github.com/omertahaoztop/focusdeck-plugins) repository, not here.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT, see [LICENSE](LICENSE).
