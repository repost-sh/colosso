#!/bin/sh
# No native menu shows on the person's screen for a right-click that isn't the person's
# (docs/browser.md 14): the harness's, an agent's through the page's protocol, and one the
# agent desktop's host posts. Each must reach the page, be refused by the gate, and leave no
# menu window of the test Koyo's in the window server's list.
#
#   no-menu-for-agents.sh <pid>          the harness's right-click, then an agent's
#   no-menu-for-agents.sh <pid> posted   after a `desktop_click` with the right button on the
#                                        page, made with the Koyo tool: checks that one
#
# A menu that got past the gate would show on the person's screen, so every right-click here
# waits for the person to have been away five minutes, and `desktop_open` and `desktop_click`
# reset that clock themselves: run this five minutes after either.
. "$(dirname "$0")/lib.sh"
pid="$1"
log="$here/.superpowers/v/l/Koyo.log"
list="$here/.superpowers/v/seam-bench-winlist"
[ -x "$list" ] || cc -o "$list" "$here/script/seam-bench/winlist.c" -framework CoreGraphics -framework CoreFoundation

away() {
  idle=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')
  [ "$idle" -ge 300 ] || fail "the person was last at the keyboard ${idle} s ago: wait until it's 300"
}

# Fails if the window server lists a window of the test Koyo's at menu level.
no_menu_window() {
  if "$list" "$pid" | grep -q "pid=$pid layer=101 "; then fail "$1: a menu window of the test Koyo's is listed"; fi
}

# Fails unless the gate refused a menu since log line <from>, saying <why>.
refused() {
  tail -n +"$(($1 + 1))" "$log" | grep 'koyo::menus' | grep -q "refused.*$2" \
    || fail "$3: the gate didn't refuse it for \"$2\": $(tail -n +"$(($1 + 1))" "$log" | grep 'koyo::menus' | tail -1)"
  if tail -n +"$(($1 + 1))" "$log" | grep -q 'koyo::menus.*showing'; then fail "$3: the gate let a menu show"; fi
}

page="$(shown_page)"
[ -n "$page" ] || fail "no page is shown: open one first"
asked="window.log.filter(l=>l.startsWith('contextmenu')).length"

if [ "${2:-}" = "posted" ]; then
  tail -n 40 "$log" | grep 'koyo::menus' | tail -1 | grep -q 'refused' || fail "desktop_click: the gate's last word isn't a refusal"
  no_menu_window "desktop_click"
  echo "ok: the agent desktop's right-click is refused, and no menu window is listed"
  echo "PASS no-menu-for-agents posted"
  exit 0
fi

set -- $(page_point 150 100)
before="$(page_eval "$page" "$asked")"
from=$(wc -l < "$log")
away
probe "rclick $1 $2" > /dev/null
no_menu_window "the harness's right-click"
probe 'wait 600' > /dev/null
no_menu_window "the harness's right-click"
expect "$page" "$asked" "$((before + 1))" "the harness's right-click reaches the page, which asks for its menu"
refused "$from" "" "the harness's right-click"
echo "ok: the harness's right-click is refused, and no menu window is listed"

pane="$("$B" app tab_list '{}' | python3 -I -c 'import json,sys
for w in json.load(sys.stdin)["worktrees"]:
    for t in w["tabs"]:
        for p in t["panes"]:
            if p["kind"] == "browser" and t.get("selected"): print(p["pane"])' | head -1)"
probe 'wait 5500' > /dev/null
from=$(wc -l < "$log")
away
"$B" app browser_click "{\"pane\":$pane,\"x\":150,\"y\":100,\"button\":\"right\"}" > /dev/null
no_menu_window "an agent's right-click"
probe 'wait 600' > /dev/null
no_menu_window "an agent's right-click"
expect "$page" "$asked" "$((before + 2))" "an agent's right-click reaches the page, which asks for its menu"
refused "$from" "an agent drives the page" "an agent's right-click"
echo "ok: an agent's right-click is refused, and no menu window is listed"
if grep -q 'koyo::menus.*ERROR\|cancelled' "$log"; then fail "the debug net caught a menu that got past the gate"; fi
echo "PASS no-menu-for-agents"
