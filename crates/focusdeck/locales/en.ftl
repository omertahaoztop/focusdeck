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
help-next = Tab  next box
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
