#!/bin/sh
# A key with no character, pressed in a page's field, is the page's: an arrow moves the caret
# and types nothing. The bug: `AppKit` offers function keys (the arrows, F1 to F12) to GPUI's
# view as key equivalents before it delivers them, GPUI's view passed them to the text system,
# and the text system typed a control character into the page's field; the key itself never
# reached the page (the owner, 2026-11-16: "arrows registers as spaces").
#
# Also: a key the page leaves still reaches Koyo's bindings (F12 opens the developer tools),
# and a key typed in the page and then in the chat's composer is typed once in each.
. "$(dirname "$0")/lib.sh"

"$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":true}" > /dev/null
probe 'wait 1500' > /dev/null
page="$(shown_page)"
[ -n "$page" ] || fail "no page is shown"
probe state | grep -q 'key true' || fail "the window isn't key: relaunch (SKILL.md, Keyboard focus)"

field="document.getElementById('field')"
codes="JSON.stringify([...$field.value].map(c=>c.charCodeAt(0)))"
keys="JSON.stringify(window.keys)"

set -- $(page_point 100 44)
probe "click $1 $2" 'wait 300' 'type abc' 'wait 300' > /dev/null
probe state | grep -q 'first responder RenderWidgetHostViewCocoa' || fail "the page hasn't the keyboard"
expect "$page" "$codes" '[97,98,99]' "abc typed in the field, once each"

probe 'press 123' 'wait 300' 'press 123' 'wait 300' > /dev/null
expect "$page" "$codes" '[97,98,99]' "two left arrows type nothing"
expect "$page" "$field.selectionStart" 1 "two left arrows move the caret two back"
expect "$page" "$keys" '["a","b","c","ArrowLeft","ArrowLeft"]' "the page hears both arrows as keys"

probe 'press 124 alt' 'wait 300' > /dev/null
expect "$page" "$codes" '[97,98,99]' "option-right types nothing"
expect "$page" "$field.selectionStart" 3 "option-right moves the caret to the word's end"

probe 'press 126' 'wait 300' 'press 125' 'wait 300' 'press 115' 'wait 300' 'press 119' 'wait 300' > /dev/null
expect "$page" "$codes" '[97,98,99]' "up, down, home and end type nothing"

probe 'press 53' 'wait 300' > /dev/null
expect "$page" "$codes" '[97,98,99]' "escape, which the page leaves, types nothing"
expect "$page" "window.keys.at(-1)" Escape "the page hears escape"

probe 'press 0' 'wait 300' > /dev/null
expect "$page" "$codes" '[97,98,99,97]' "a letter after them is typed once, at the caret"

before="$(probe pages | grep -c '^page ')"
probe 'press 111' 'wait 1500' > /dev/null
expect "$page" "window.keys.at(-1)" F12 "the page hears F12 first"
expect "$page" "$codes" '[97,98,99,97]' "F12 types nothing"
after="$(probe pages | grep -c '^page ')"
[ "$after" -gt "$before" ] || fail "F12, left by the page, didn't open the developer tools ($before pages, then $after)"
echo "ok: F12, left by the page, reaches Koyo's binding: the developer tools opened"

if grep -q 'panicked' "$here/.superpowers/v/l/Koyo.log"; then fail "the log has a panic"; fi
echo "PASS arrows-in-page"
