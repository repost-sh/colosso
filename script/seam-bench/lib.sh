# Shared by the seam's bench sequences (docs/native-layer.md 9.3). Each sequence drives the
# test Koyo of this checkout, already opened on the agent desktop and confirmed
# (.agents/skills/verify/SKILL.md, Launch), through its probe, and fails on the first thing
# that isn't as it should be.
set -eu
here="$(cd "$(dirname "$0")/../.." && pwd)"
B="$here/.agents/skills/verify/bin/koyo-verify"
fixture="file://$here/.superpowers/v/fixture/focus.html"

probe() { "$B" probe "$@"; }

fail() { echo "FAIL: $*" >&2; exit 1; }

# The host's id of the page a pane draws, the latest made.
shown_page() {
  probe pages | awk '/drawn by a pane true/ { sub(":", "", $2); page = $2 } END { print page }'
}

# A point of the shown page, <dx> <dy> from its top left, in the window's points from its top
# left: from Koyo's window and the page's holder, both in screen points from the bottom left.
page_point() {
  probe windows | awk -v dx="$1" -v dy="$2" '
    function frame(line,    n, part, at, size) {
      n = match(line, /-?[0-9]+,-?[0-9]+ [0-9]+x[0-9]+ visible/)
      split(substr(line, RSTART, RLENGTH), part, " ")
      split(part[1], at, ","); split(part[2], size, "x")
      x = at[1]; y = at[2]; w = size[1]; h = size[2]
    }
    / GPUIWindow / { frame($0); kx = x; ky = y; kh = h }
    / through true / { frame($0); if (x >= 0 && y >= 0) { hx = x; hy = y; hh = h } }
    END { print hx - kx + dx, (ky + kh) - (hy + hh) + dy }'
}

# What <script> evaluates to in page <page>: its value alone, a string's as it is.
page_eval() {
  probe "eval $1 $2" | python3 -I -c '
import json, sys
line = sys.stdin.read().split(" => ", 1)[1]
print(json.loads(line)["result"].get("value"))'
}

# Fails unless page <page>'s <script> gives <want>, saying <what> it checks.
expect() {
  got="$(page_eval "$1" "$2")"
  [ "$got" = "$3" ] || fail "$4: got $got, wanted $3"
  echo "ok: $4"
}
