# Quick-capture words: space-separated spellings the parser accepts, case-insensitive.
capture-words-tomorrow = tomorrow
capture-words-minute = min m
capture-words-hour = h
capture-words-mon = mon monday
capture-words-tue = tue tuesday
capture-words-wed = wed wednesday
capture-words-thu = thu thursday
capture-words-fri = fri friday
capture-words-sat = sat saturday
capture-words-sun = sun sunday

# Bottom hint line on the main screen: key and action, joined with " · ".
hint-board-note = a note
hint-board-select = click select
hint-board-zoom = double click zoom
hint-board-drag = drag move
hint-board-box = Tab box
hint-board-help = ? help
hint-board-quit = q quit
# Hints shared by the main screen and the zoomed box.
hint-open = o open
hint-move = m move
hint-action-done = x done
hint-action-unstar = u unstar
hint-zoom-plan = space add to plan
hint-zoom-search = / search
hint-zoom-back = Esc back
hint-capture = Enter save · Esc cancel · @person #label R123 · at the end 14:30, tomorrow 09:00, 30min
hint-picker = 1-9 target box · Esc cancel
hint-search = type to search · Enter done · Esc clear
hint-help = Esc close

# Hint while an item is being dragged: what releasing the mouse does.
drag-into = drop: { $title }
drag-reorder = drop: row { $row }
drag-rejected = { $title } does not accept
drag-nothing = nothing happens if you drop
drag-cancel = { $what } · Esc cancel

# Key list opened with "?", one key per line, keys padded with two spaces.
help-title = Keys
help-note = a  take a note (into the capture_into box)
help-done = x  mark the selected item done
help-unstar = u  unstar the selected item
help-move = m  move to another box
help-plan = space  add to / remove from plan (zoomed box)
help-open = o  open the link in the browser
help-search = /  search (zoomed box)
help-next = Tab, arrows  switch box (↑↓ at the list edge)
help-zoom = Enter  zoom the box · Esc back
help-quit = q  quit

# Box title suffixes: item count, planned items over total, overdue items.
box-count = { $total }
box-planned = { $planned } / { $total }
box-overdue = { $count } overdue
box-error = error: { $message }

picker-title = Move to
capture-title = Note

status-refreshed = refreshed { $time }
status-not-refreshed = not refreshed yet

# Messages flashed on the top line.
flash-action-failed = { $what } failed: { $message }
flash-no-done = this box has no done action
flash-no-unstar = this box has no unstar action
flash-no-link = this item has no link
flash-drop-rejected = { $title } box does not accept the dropped item
flash-box-closed = { $title } box is not open
flash-box-busy = { $title } box is busy
flash-box-stopped = the plugin task of the { $title } box stopped
flash-no-capture-into = the profile has no capture_into
flash-capture-failed = could not add the note: { $message }
flash-open-failed = could not start xdg-open: { $error }

terminal-raw-mode = could not put the terminal in raw mode
terminal-alt-screen = could not switch to the alternate screen
terminal-event = could not read the terminal event

# Command line help: clap prints its own "Usage:", "Options:" and the help flag text.
cli-about = Focus board
cli-profile-help = Profile name to use
cli-name-value = NAME
cli-text-value = TEXT
cli-capture-about = Adds the text to the capture_into box of the profile
cli-remind-about = Lists upcoming items
cli-remind-send-help = Send the notification
cli-plugin-about = Installs, lists and removes plugins
cli-plugin-install-about = Installs a plugin from the registry
cli-plugin-list-about = Lists installed plugins
cli-plugin-remove-about = Removes a plugin

cli-installed = { $name } { $version } installed
cli-removed = { $name } removed
cli-in-use-warning = warning: profile { $profile } uses this plugin
cli-latest = (in registry: { $version })
cli-not-in-registry = (in registry: none)
cli-registry-unreachable = registry unreachable
cli-builtin = (built-in)

error-prefix = error:
error-duration-invalid = invalid duration: { $value } (example: 90s, 2m, 1h)
error-duration-zero = duration cannot be zero: { $value }
error-plugin-not-installed = { $plugin } is not installed, run focusdeck plugin install { $plugin }
error-secret-missing = { $path } not found
error-secret-exposed = { $path } is readable by others, run chmod 600
error-secret-unreadable = could not check { $path }: { $error }
error-no-base-dir = { $var } is not set and no home directory was found

config-read-failed = could not read { $path }
config-invalid = { $path } is invalid

profile-read-failed = could not read { $path }: { $error }
profile-parse-failed = { $path } is invalid: { $error }
profile-duplicate-box-id = duplicate box id: { $id }
profile-unknown-capture-into = capture_into points to an unknown box: { $id }
profile-plan-into-self = box { $id } cannot use itself as plan_into
profile-unknown-plan-into = plan_into points to an unknown box: { $id }
profile-mixed-servers = { $plugin } boxes point to different servers: { $first }, { $second }
profile-empty-box = width and height of box { $id } must be at least 1
profile-overlapping-cells = boxes { $first } and { $second } share a cell: row { $row } column { $col }
profile-no-profile = no profile selected: pass --profile or set default_profile in config.toml
profile-invalid-name = invalid profile name: { $name }
profile-create-failed = could not create { $path }: { $error }

install-invalid-name = invalid plugin name: { $name }
install-checksum-mismatch = checksum mismatch
install-checksum-mismatch-named = checksum mismatch: { $name }
install-file-url-relative = file:// address must be an absolute path: { $url }
install-read-failed = could not read { $path }
install-unsupported-url = unsupported address (only http, https, file): { $url }
install-file-too-large = file too large
install-too-many-redirects = too many redirects
install-insecure-url = refusing plain http (use https): { $url }
install-http-client = could not build the HTTP client
install-download-failed = could not download { $url }
install-http-status = could not download { $url }: HTTP { $status }
install-url-too-large = { $url } is too large
install-index-not-utf8 = registry is not UTF-8
install-index-invalid = registry is invalid
install-not-found = no such plugin in the registries: { $name }
install-not-found-unreachable = no such plugin in the registries: { $name } (unreachable registries: { $failures })
install-protocol-mismatch = { $name } wants protocol { $wanted }, the core knows { $known }
install-no-target = no build of { $name } for this machine ({ $target })
install-dir-create-failed = could not create { $path }
install-replace-failed = could not move the plugin into place
install-write-failed = could not write the plugin
install-sync-failed = could not flush the plugin to disk
install-chmod-failed = could not make the plugin executable
install-manifest-failed = could not write the manifest
install-delete-failed = could not delete { $path }
install-not-installed = plugin is not installed: { $name }
install-local-builtin = local comes with focusdeck, there is nothing to install
install-local-not-removable = local comes with focusdeck and cannot be removed
install-remote-file-target = the index at { $registry } points { $name } at a local file
install-binary-changed = { $name } no longer matches its manifest; run "focusdeck plugin install { $name }" again

# Notification titles and bodies of "focusdeck capture" and the rofi prompt label.
capture-prompt = Note
capture-added-title = added
capture-failed-title = focusdeck: not added
capture-queued-title = focusdeck: queued
capture-lost-title = focusdeck: note may be lost
capture-queue-flushed =
    { $count ->
        [one] { $count } note
       *[other] { $count } notes
    } sent from the queue
capture-no-capture-into = this profile has no capture_into
capture-empty-title = title is empty
capture-command-empty = capture_command is empty
capture-prompt-spawn-failed = could not start { $program }: { $error }
capture-no-box = no box named { $target }

remind-write-failed = could not write { $path }

host-spawn-failed = could not start the plugin: { $error }
host-timeout = plugin did not answer within 15 s
host-crashed = plugin exited
host-bad-line = plugin wrote an invalid line: { $line }
host-protocol-mismatch = protocol mismatch: core { $expected }, plugin { $got }
host-invalid-result = invalid result: { $error }
host-invalid-describe = invalid describe: { $error }
host-non-utf8 = output is not UTF-8
host-line-too-long = plugin sent a line longer than 16 MiB

local-file-corrupt = { $path } could not be read ({ $error }); focusdeck will not write to it
local-file-newer = { $path } was written by a newer focusdeck (format { $version }); update focusdeck
local-file-io = { $path }: { $error }
local-invalid-settings = invalid local box settings: { $error }
local-item-missing = note { $id } is not in this box
local-list-required = `list` must be a non-empty string
local-position-required = a position is required
local-unknown-action = unknown action: { $action }

starter-title = Notes
starter-tmp-left = could not remove temporary file { $path }: { $error }
