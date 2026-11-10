// Koyo Agent Desktop's executable (docs/agent-desktop.md 3): a loader that never changes, so the
// privacy grants the person gives it (Accessibility, Screen Recording), which an ad-hoc signature
// ties to this binary's hash, survive every build of the host. It loads the one library that
// lies beside its bundle, wherever the bundle is, and hands it the process's arguments.
#include <dlfcn.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define LIBRARY "libkoyo_desk_host.dylib"

int main(int argc, char **argv) {
  char executable[PATH_MAX];
  uint32_t size = sizeof executable;
  char folder[PATH_MAX];
  if (_NSGetExecutablePath(executable, &size) != 0 || !realpath(executable, folder)) {
    fprintf(stderr, "Koyo Agent Desktop: no path to itself\n");
    return 70;
  }
  // <folder>/Koyo Agent Desktop.app/Contents/MacOS/<executable>: four names up.
  for (int up = 0; up < 4; up++) {
    char *slash = strrchr(folder, '/');
    if (!slash) return 70;
    *slash = '\0';
  }
  char library[PATH_MAX];
  if (snprintf(library, sizeof library, "%s/%s", folder, LIBRARY) >= (int)sizeof library) return 70;
  void *handle = dlopen(library, RTLD_NOW | RTLD_LOCAL);
  if (!handle) {
    fprintf(stderr, "Koyo Agent Desktop: %s\n", dlerror());
    return 71;
  }
  int (*host_main)(int, char **) = (int (*)(int, char **))dlsym(handle, "koyo_desk_host_main");
  if (!host_main) {
    fprintf(stderr, "Koyo Agent Desktop: %s has no entry\n", library);
    return 72;
  }
  return host_main(argc, argv);
}
