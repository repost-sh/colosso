#!/bin/sh
# The menu bar's Edit menu takes no key from what has the keyboard (docs/native-layer.md 7):
# it's there with the system's six edits and no key of its own on any of them, the edit keys
# still do their work in a page's field, and a key Koyo binds nothing
# to in a page still reaches the page. The menu is read, never opened.
. "$(dirname "$0")/lib.sh"

bar="$(probe menus | sed 's/^menus => //')"
echo "$bar" | grep -q '\[Edit\] Undo; Redo; Cut; Copy; Paste; Select All;' \
  || fail "no Edit menu with the six edits, none with a key of its own: $bar"
echo "ok: the Edit menu has Undo, Redo, Cut, Copy, Paste and Select All, none with a key of its own"
echo "    $(echo "$bar" | sed 's/.*\[Edit\]/[Edit]/')"

"$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":true}" > /dev/null
probe 'wait 1500' > /dev/null
page="$(shown_page)"
[ -n "$page" ] || fail "no page is shown"
field="document.getElementById('field')"

# With the app in front, `AppKit` offers a ⌘ key to Koyo's bindings before the page hears it.
# Behind, as a test Koyo is once it has lost the front, the key reaches the page too, until the
# layer routes keys at the window (docs/native-layer.md 4.1, step 5): not this sequence's to show.
probe state | grep -q 'app active true' || fail "the test Koyo isn't the app in front: relaunch, and run this first"

set -- $(page_point 100 44)
probe "click $1 $2" 'wait 300' 'type abc' 'wait 300' > /dev/null
expect "$page" "$field.value" abc "abc typed in the page's field"

probe 'press 0 cmd' 'wait 400' > /dev/null
expect "$page" "$field.selectionEnd - $field.selectionStart" 3 "command-A selects the field's text, through Koyo's edit"
# No cut, copy or paste here: the clipboard is the person's own, and a test neither writes it
# nor reads it.
probe 'type x' 'wait 300' > /dev/null
expect "$page" "$field.value" x "a letter typed over the selection replaces it"
probe 'press 6 cmd' 'wait 400' > /dev/null
expect "$page" "$field.value" abc "command-Z undoes that, through Koyo's edit"

probe 'press 40 cmd' 'wait 400' > /dev/null
expect "$page" "JSON.stringify(window.keys)" '["a","b","c","x","k"]' "command-K, which Koyo binds nothing to, reaches the page as a key, and command-A and command-Z before it, which Koyo took, never did"
expect "$page" "$field.value" abc "and types nothing"
echo "PASS edit-menu"
