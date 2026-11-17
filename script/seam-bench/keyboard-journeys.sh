#!/bin/sh
# The keyboard goes where the person puts it, between a page and Koyo's own parts, and stays
# there when something else asks for it (docs/keyboard.md 9, the journeys a test Koyo on the
# agent desktop can make):
#
#   1   a page, then a chat's composer, then the page again: each typed in, each got its own
#   1b  back in the page's tab, a click 200 ms after it's shown and a key 50 ms after the
#       click: the page hears the key (for a second after its views come back, the view under
#       the pointer is the page's outer one, which takes the press and never asks for the
#       keyboard; a page has it from the press that reaches it, docs/native-layer.md 11.1)
#   1c  a press on Koyo's own edge beside the page gives the page nothing
#   1d  (`keyboard-journeys.sh right`, run apart, the person away five minutes) the right
#       button on a page gives it the keyboard as the left does; its menu is refused
#   3   the page's developer tools, docked with ⌥⌘I while the page has the keyboard: it stays
#       the page's; a click in the tools gives them the keyboard, and what's typed there isn't
#       the page's; ⌥⌘I again, out of the tools, gives it back to their page
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

# Journey 1d, by itself: a right-click asks the page for its menu, which the gate refuses
# for a window on the agent desktop; like every right-click here, only with the person away.
if [ "${1:-}" = "right" ]; then
  idle=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')
  [ "$idle" -ge 300 ] || { echo "the person was last at the keyboard ${idle} s ago: wait until it's 300" >&2; exit 1; }
  pane="$("$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":true}" | pane_of)"
  probe 'wait 1500' > /dev/null
  page="$(shown_page)"
  [ -n "$page" ] || { echo "no page is shown" >&2; exit 1; }
  "$B" app tab_open '{"kind":"chat","select":true}' > /dev/null
  probe 'wait 1000' > /dev/null
  "$B" app tab_select "{\"pane\":$pane}" > /dev/null
  probe 'wait 1500' > /dev/null
  held_by 'gpui ([^,]*)' gpui "1d: a tab shown through app control leaves the keyboard with GPUI"
  set -- $(page_point 300 400)
  probe "rclick $1 $2" 'wait 400' > /dev/null
  held_by "page $page" "page $page" "1d: the right button on the page gives it the keyboard"
  if grep -q 'koyo::menus.*showing' "$log"; then fail "1d: the gate let a menu show"; fi
  finish "keyboard-journeys right"
  exit 0
fi

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

# Journey 1b.
"$B" app tab_select "{\"pane\":$chat_pane}" > /dev/null
probe 'wait 800' "click $composer_x $composer_y" 'wait 400' 'type c' 'wait 300' > /dev/null
"$B" app tab_select "{\"pane\":$page_pane}" > /dev/null
probe 'wait 200' "click $field_x $field_y" 'wait 50' 'type t' 'wait 500' > /dev/null
expect "$page" "window.keys.at(-1)" t "1b: a key 50 ms after a click 200 ms after the tab is shown is the page's"
held_by "page $page" "page $page" "1b: and the page has the keyboard"

# Journey 1c.
"$B" app tab_select "{\"pane\":$chat_pane}" > /dev/null
probe 'wait 600' > /dev/null
"$B" app tab_select "{\"pane\":$page_pane}" > /dev/null
probe 'wait 1500' > /dev/null
held_by 'gpui ([^,]*)' gpui "1c: a tab shown through app control leaves the keyboard with GPUI"
edge_x=$((field_x - 100 - 3)) edge_y=$((field_y + 300))
if probe "hover $edge_x $edge_y" | grep -q 'over GPUI'; then
  heard="$(page_eval "$page" 'window.keys.length')"
  probe "click $edge_x $edge_y" 'wait 400' 'type u' 'wait 300' > /dev/null
  held_by 'gpui ([^,]*)' gpui "1c: a press on Koyo's edge beside the page leaves the keyboard with GPUI"
  expect "$page" "window.keys.length" "$heard" "1c: and a key after it isn't the page's"
else
  fail "1c: the point left of the page isn't Koyo's own: $(probe "hover $edge_x $edge_y")"
fi

# Journey 3.
probe "click $field_x $field_y" 'wait 400' > /dev/null
held_by "page $page" "page $page" "3: the page has the keyboard"
before="$(probe pages | grep -c '^page ')"
probe 'press 34 alt cmd' > /dev/null
# The tools are a page of their own, made and loaded after the key: up to ten seconds.
tools=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  probe 'wait 1000' > /dev/null
  tools="$(probe pages | awk -v page="$page" '/drawn by a pane true/ { sub(":", "", $2); if ($2 != page) tools = $2 } END { print tools }')"
  [ -z "$tools" ] || break
done
probe 'wait 1000' > /dev/null
if [ -n "$tools" ] && [ "$(probe pages | grep -c '^page ')" -gt "$before" ]; then
  echo "ok: 3: option-command-I docks the developer tools, as page $tools"
  held_by "page $page" "page $page" "3: and the keyboard stays the page's as they open"
  # The tools' holder is the one on screen whose top left isn't the page's own.
  set -- $(probe windows | awk '
    function frame(line,    part, at, size) {
      match(line, /-?[0-9]+,-?[0-9]+ [0-9]+x[0-9]+ visible/)
      split(substr(line, RSTART, RLENGTH), part, " ")
      split(part[1], at, ","); split(part[2], size, "x")
      x = at[1]; y = at[2]; w = size[1]; h = size[2]
    }
    / GPUIWindow / { frame($0); kx = x; ky = y; kh = h }
    / through true / { frame($0); if (x >= 0 && y >= 0) { n++; hx[n] = x; hy[n] = y; hw[n] = w; hh[n] = h } }
    END {
      # The page keeps its top left as the tools dock right of it or under it.
      t = 1
      for (i = 2; i <= n; i++) if (hx[i] > hx[t] || hy[i] + hh[i] < hy[t] + hh[t]) t = i
      if (n < 2) exit
      print int(hx[t] - kx + hw[t] / 2), int((ky + kh) - (hy[t] + hh[t]) + hh[t] / 2)
    }')
  if [ "$#" -eq 2 ]; then
    heard="$(page_eval "$page" 'window.keys.length')"
    probe "click $1 $2" 'wait 500' > /dev/null
    held_by "page $tools" "page $tools" "3: a click in the tools gives them the keyboard"
    probe 'type q' 'wait 300' > /dev/null
    expect "$page" "window.keys.length" "$heard" "3: a key typed in the tools isn't the page's"
    # The tools' surface goes a frame or more after the key, and the window puts its views
    # back then: three seconds, for the state it settles in.
    probe 'press 34 alt cmd' 'wait 3000' > /dev/null
    held_by "page $page" "page $page" "3: option-command-I out of the tools gives the keyboard back to their page"
    probe 'type r' 'wait 300' > /dev/null
    expect "$page" "window.keys.at(-1)" r "3: and the page hears the next key"
  else
    fail "3: the tools' place in the window wasn't found: $(probe windows | grep 'through true')"
  fi
else
  fail "3: option-command-I didn't dock the developer tools: $(probe pages | tr '\n' ' ')"
fi

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
