# Koyo's patches to Chromium and CEF

`script/cef` applies every `*.patch` here, in name order, to Chromium's sources (CEF's own
checkout is `cef/` inside them) after CEF's own patches, and builds the framework with them.
What each is for, and what extensions get from it, is `docs/extensions.md` 10. The framework
lists the patches it was built with in `Resources/koyo-patches.txt`; the host
(`koyo-rs/koyo-browser-host/src/extensions.rs`) reads that before it asks the engine for
anything a patch adds.

| Patch | From | Touches |
| --- | --- | --- |
| `devtools-pipe-descriptors` | Koyo | `content/browser/devtools/devtools_agent_host_impl.cc` |
| `dialog-without-parent-titlebar` | Koyo | `ui/views/widget/native_widget_mac.mm` |
| `extension-current-window-last-active` | Koyo | `chrome/browser/extensions/window_controller_list.cc` |
| `extension-install-ui-null-tab` | Cra | `chrome/browser/ui/extensions/extension_install_ui_desktop.cc` |
| `extension-popup-browser-info` | Cra | `cef/libcef/browser/browser_info_manager.cc` |
| `extension-popup-no-popout-wait` | Cra | `chrome/browser/ui/views/extensions/extensions_toolbar_desktop.cc` |
| `extension-tab-activate` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extension-tabs-create` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extensions-normal-windows` | Cra | `chrome/browser/extensions/api/extension_action/`, `chrome/browser/extensions/browser_extension_window_controller.cc` |
| `koyo-extensions-protocol` | Koyo, parts after Cra | the `Extensions` DevTools domain (`Extensions.pdl`, `extensions_handler.{h,cc}`, `inspector_protocol_config.json`), `cef/libcef/browser/chrome/browser_delegate.h`, `chrome_browser_delegate.cc` |

- **Order matters where two touch a file.** `koyo-extensions-protocol` goes over
  `extension-tab-activate` and `extension-tabs-create` in CEF's browser delegate; the names
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
