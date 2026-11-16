// Lists the window server's windows: every window (kCGWindowListOptionAll) and, marked, those
// on screen now, i.e. on the Spaces the person sees (kCGWindowListOptionOnScreenOnly).
// Usage: winlist [pid]   with a pid, only that process's windows, plus every window at menu
// level (layer >= 101) of any process on screen.
#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>
#include <stdio.h>
#include <stdlib.h>

static long num(CFDictionaryRef d, CFStringRef k) {
  long v = 0; CFNumberRef n = CFDictionaryGetValue(d, k);
  if (n) CFNumberGetValue(n, kCFNumberLongType, &v);
  return v;
}
static void str(CFDictionaryRef d, CFStringRef k, char *out, size_t len) {
  out[0] = 0; CFStringRef s = CFDictionaryGetValue(d, k);
  if (s) CFStringGetCString(s, out, len, kCFStringEncodingUTF8);
}
int main(int argc, char **argv) {
  long only = argc > 1 ? atol(argv[1]) : 0;
  CFArrayRef on = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);
  CFArrayRef all = CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID);
  CFMutableSetRef shown = CFSetCreateMutable(NULL, 0, NULL);
  for (CFIndex i = 0; i < CFArrayGetCount(on); i++)
    CFSetAddValue(shown, (void *)num(CFArrayGetValueAtIndex(on, i), kCGWindowNumber));
  for (CFIndex i = 0; i < CFArrayGetCount(all); i++) {
    CFDictionaryRef w = CFArrayGetValueAtIndex(all, i);
    long pid = num(w, kCGWindowOwnerPID), layer = num(w, kCGWindowLayer), n = num(w, kCGWindowNumber);
    int is_on = CFSetContainsValue(shown, (void *)n);
    if (only && pid != only && !(is_on && layer >= 101)) continue;
    char owner[256], name[256]; str(w, kCGWindowOwnerName, owner, sizeof owner); str(w, kCGWindowName, name, sizeof name);
    CGRect r = CGRectZero; CFDictionaryRef b = CFDictionaryGetValue(w, kCGWindowBounds);
    if (b) CGRectMakeWithDictionaryRepresentation(b, &r);
    printf("%s pid=%ld layer=%ld win=%ld owner=\"%s\" name=\"%s\" %.0f,%.0f %.0fx%.0f\n", is_on ? "ON " : "off",
           pid, layer, n, owner, name, r.origin.x, r.origin.y, r.size.width, r.size.height);
  }
  return 0;
}
