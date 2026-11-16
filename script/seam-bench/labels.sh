#!/bin/sh
# Each way an input comes in leaves its own label in the native layer's journal
# (docs/native-layer.md 5): the harness's press handed to the window, the harness's key put
# in the app's queue, and an agent's click through the page's protocol. A click the agent
# desktop's host posts is the fourth: it's made with the Koyo tool `desktop_click` between
# two runs of this script, which then checks it with `labels.sh posted`.
. "$(dirname "$0")/lib.sh"

if [ "${1:-}" = "posted" ]; then
  seam | grep -q 'window [0-9]*: Posted(DeskHost) Press' \
    || seam | grep -q 'window [0-9]*: Person { through: Some' \
    || fail "a desktop_click isn't kept as posted: $(seam)"
  echo "ok: the agent desktop's click is kept as posted, or as a tool's while this Koyo knows no desk host: $(seam | tr ';' '\n' | grep window)"
  echo "PASS labels posted"
  exit 0
fi

pane="$("$B" app tab_open "{\"kind\":\"browser\",\"url\":\"$fixture\",\"viewport\":{\"fill\":true},\"select\":true}" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["pane"])')"
probe 'wait 1500' > /dev/null
page="$(shown_page)"
[ -n "$page" ] || fail "no page is shown"

set -- $(page_point 100 44)
probe "click $1 $2" 'wait 300' > /dev/null
seam | grep -q 'window [0-9]*: Probe Press' || fail "the harness's press isn't kept as its own: $(seam)"
echo "ok: a press the harness hands to the window is kept as the harness's"

probe 'press 0' 'wait 300' > /dev/null
seam | grep -q 'window [0-9]*: Probe Key' || fail "the harness's queued key isn't kept as its own: $(seam)"
echo "ok: a key the harness queues is kept as the harness's, through the application's dispatch"

seam | grep -q "page $page: no engine input" || fail "page $page has engine input before any: $(seam)"
probe 'wait 5500' > /dev/null
"$B" app browser_click "{\"pane\":$pane,\"x\":100,\"y\":44}" > /dev/null
seam | grep -q "page $page: Engine" || fail "an agent's click isn't kept for page $page: $(seam)"
seam | grep -q 'window [0-9]*: Probe Key' || fail "an agent's click changed the window's last input: $(seam)"
echo "ok: an agent's click through the page's protocol is kept for its page, and leaves the window's last input as it was"
echo "PASS labels"
