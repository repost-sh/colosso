# Koyo's patches to Chromium and CEF

`script/cef` applies every `*.patch` here, in name order, to Chromium's sources (CEF's own
checkout is `cef/` inside them) after CEF's own patches, and builds the framework with them.
What each is for, and what extensions get from it, is `docs/extensions.md` 10. The framework
lists the patches it was built with, each with a hash of its content, in
`Resources/koyo-build.txt` (`script/cef-framework`); the host
(`koyo-rs/koyo-browser-host/src/framework.rs`) reads that before it asks the engine for
anything a patch adds, and `script/browser-engine` packages a release engine only from a
framework built with exactly these. The sources keep a copy of each patch applied
(`out/koyo-patches`), so a patch changed or removed here stops `script/cef` with how to take
the old one out, rather than building it beside the new.

| Patch | From | Touches |
| --- | --- | --- |
| `devtools-pipe-descriptors` | Koyo | `content/browser/devtools/devtools_agent_host_impl.cc` |
| `devtools-pipe-embedder` | Koyo | `chrome/browser/devtools/chrome_devtools_manager_delegate.cc`, `remote_debugging_server.cc` |
| `dialog-without-parent-titlebar` | Koyo | `ui/views/widget/native_widget_mac.mm` |
| `external-pump-idle-work` | CEF upstream `b7f5636` (issue 4332), not yet in 156.0.14 | `cef/libcef/browser/browser_message_loop.cc`: an external pump runs idle work whenever no immediate work is left, which accepting an autofill suggestion waits on |
| `external-pump-wake-ups` | Koyo | `cef/libcef/browser/browser_message_loop.cc`: a turn of an external pump ends by asking for the wakeup Chromium's last task left it, as Chromium's own pumps do, so work left when the turn ran out of time and work put off from within a task ask for their turn, and the host's pump runs CEF's work only when it's asked (`docs/native-pages.md` 3.1). CEF has no such change, to 2026-11-20; its own example pump runs the work every thirtieth of a second instead (CEF issue 2968) |
| `extension-current-window-last-active` | Koyo | `chrome/browser/extensions/window_controller_list.cc` |
| `extension-install-ui-null-tab` | Cra | `chrome/browser/ui/extensions/extension_install_ui_desktop.cc` |
| `extension-popup-browser-info` | Cra | `cef/libcef/browser/browser_info_manager.cc` |
| `extension-popup-no-popout-wait` | Cra | `chrome/browser/ui/views/extensions/extensions_toolbar_desktop.cc` |
| `extension-tab-activate` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extension-tabs-create` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extensions-normal-windows` | Cra | `chrome/browser/extensions/api/extension_action/`, `chrome/browser/extensions/browser_extension_window_controller.cc` |
| `koyo-extensions-protocol` | Koyo, parts after Cra | the `Extensions` DevTools domain (`Extensions.pdl`, `extensions_handler.{h,cc}`, `inspector_protocol_config.json`), `cef/libcef/browser/chrome/browser_delegate.h`, `chrome_browser_delegate.cc` |
| `koyo-popup` | Koyo | the `Extensions` domain again (`koyoActionPointerDown`, the button `koyoTriggerAction` takes), `chrome/browser/ui/extensions/extension_popup_types.h`, `extension_action_delegate.h`, `chrome/browser/ui/views/extensions/extension_action_delegate_desktop.{h,cc}`, `extension_popup.{h,cc}`, `extension_view_views.{h,cc}`, `ui/views/bubble/bubble_dialog_delegate_view.{h,cc}` |

- **Order matters where two touch a file.** `koyo-extensions-protocol` goes over
  `extension-tab-activate` and `extension-tabs-create` in CEF's browser delegate,
  `koyo-popup` over `koyo-extensions-protocol` in the `Extensions` domain's files, and
  `external-pump-wake-ups` over `external-pump-idle-work` in CEF's message loop; the names
  sort that way.
- **Made against** CEF 156.0.14 (`4fa4ab5`) on Chromium 156.0.8078.48. On another version
  each has to apply again: `script/cef` stops with the name of one that doesn't.
- **Cra's** are from `github.com/VictorKildahl/cra` (`scripts/cef-patches`), a macOS browser
  that embeds Chrome-style CEF pages as Koyo does, under the MIT licence below; their comments
  say "Cra" where they tell why. Changed here: the marker of an extension's tab is
  `koyo_extension_tab`, and the message that asks for a tab in front is `Koyo.activate`.
  Cra's `extension-trigger-action-null-checks` and the icon and badge part of its
  `extensions-action-updates` are inside `koyo-extensions-protocol`, reworked.

## Cra's licence

MIT License

Copyright (c) 2026 Victor Kildahl

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
