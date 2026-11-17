#!/bin/sh
# The keyboard goes where the person puts it, between a page and Koyo's own parts, and stays
# there when something else asks for it (docs/keyboard.md 9, the journeys a test Koyo on the
# agent desktop can make):
#
#   1   a page, then a chat's composer, then the page again: each typed in, each got its own
#   2   a page that opens and loads by itself while the composer has the keyboard: the composer
#       keeps it
#   11  an agent types into its page while the composer has the keyboard: the composer keeps it
#
# What's typed in the page is read from the page's own record of its keys. What's typed in the
# composer isn't read back yet: that it went there is told by the page's record staying as it
# was and by GPUI's view having the keyboard. Nothing is sent to a chat, and nothing is cut,
# copied or pasted. The count of the keyboard's backstops (a page refused, a first responder
# healed) is printed at the end.
. "$(dirname "$0")/lib.sh"
log="$here/.superpowers/v/l/Koyo.log"

pane_of() { python3 -I -c 'import json,sys; print(json.load(sys.stdin)["pane"])'; }
keys="JSON.stringify(window.keys)"

# Fails unless the keyboard is <holder>'s, the first responder with it, in a key window.
held_by() {
  said="$(probe keyboard | sed 's/^keyboard => //')"
  if echo "$said" | grep -q "holder $1, first responder $2, key true"; then echo "ok: $3"; else fail "$3: $said"; fi
}

probe state | grep -q 'app active true' || fail "the test Koyo isn't the app in front: relaunch, and run this first"

page_pane="$("$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":true}" | pane_of)"
probe 'wait 1500' > /dev/null
page="$(shown_page)"
[ -n "$page" ] || fail "no page is shown"
set -- $(page_point 100 44)
field_x=$1 field_y=$2

# Journey 1.
probe "click $field_x $field_y" 'wait 300' 'type ab' 'wait 300' > /dev/null
expect "$page" "$keys" '["a","b"]' "1: typed in the page's field, the page hears each key once"
held_by "page $page" "page $page" "1: the page has the keyboard after a click in it"

chat_pane="$("$B" app tab_open '{"kind":"chat","select":true}' | pane_of)"
probe 'wait 1200' > /dev/null
set -- $(probe 'bounds chat-composer' | sed 's/^bounds chat-composer => //')
[ "$#" -ge 4 ] || fail "the composer isn't drawn: $(probe 'bounds chat-composer')"
composer_x=$(echo "$1 $3" | awk '{print int($1 + $2 / 4)}')
composer_y=$(echo "$2 $4" | awk '{print int($1 + $2 / 3)}')
probe "click $composer_x $composer_y" 'wait 400' > /dev/null
held_by 'gpui ([^,]*)' gpui "1: GPUI's view has the keyboard after a click in the composer"
probe 'type cd' 'wait 300' > /dev/null
expect "$page" "$keys" '["a","b"]' "1: typed in the composer, none of it reaches the page left behind"

"$B" app tab_select "{\"pane\":$page_pane}" > /dev/null
probe 'wait 800' "click $field_x $field_y" 'wait 300' 'type e' 'wait 300' > /dev/null
expect "$page" "$keys" '["a","b","e"]' "1: back in the page, it hears the next key"
held_by "page $page" "page $page" "1: and has the keyboard again"

# Journey 2.
"$B" app tab_select "{\"pane\":$chat_pane}" > /dev/null
probe 'wait 800' "click $composer_x $composer_y" 'wait 400' > /dev/null
held_by 'gpui ([^,]*)' gpui "2: the composer has the keyboard"
"$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":false}" > /dev/null
probe 'wait 2500' > /dev/null
held_by 'gpui ([^,]*)' gpui "2: a page that opened and loaded by itself didn't take it"
probe 'type f' 'wait 300' > /dev/null
others="$(probe pages | awk '/^page / { sub(":", "", $2); print $2 }')"
for other in $others; do
  heard="$(page_eval "$other" "$keys")"
  case "$heard" in *f*) fail "2: page $other heard the key typed in the composer: $heard" ;; esac
done
echo "ok: 2: no page heard the key typed in the composer"

# Journey 11.
probe 'wait 5500' > /dev/null
"$B" app browser_type "{\"pane\":$page_pane,\"selector\":\"#field\",\"text\":\"zz\"}" > /dev/null
probe 'wait 600' > /dev/null
expect "$page" "String(document.getElementById('field').value.endsWith('zz'))" true "11: an agent's text lands in its page's field"
held_by 'gpui ([^,]*)' gpui "11: and the composer keeps the keyboard"
probe 'type g' 'wait 300' > /dev/null
expect "$page" "String(window.keys.includes('g'))" false "11: a key typed in the composer after it doesn't reach the agent's page"

refused=$(grep -c 'koyo::keyboard.*refused' "$log" || true)
healed=$(grep -c 'koyo::keyboard.*healed' "$log" || true)
if grep -q 'panicked' "$log"; then fail "the log has a panic"; fi
echo "backstops: $refused refused, $healed healed"
finish keyboard-journeys
