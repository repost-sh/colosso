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
| `content-client-goes-with-its-delegate` | Koyo | `cef/libcef/browser/main_runner.cc`: content's client is cleared as CEF's main delegate, which owns it, is destroyed, so a process that runs on after the engine stops (Koyo, its engine's tests) has none left to read; an occlusion update `WebContentsOcclusionCheckerMac` put off read the freed one and crashed the process, on CEF 158 |
| `devtools-pipe-descriptors` | Koyo | `content/browser/devtools/devtools_agent_host_impl.cc` |
| `devtools-pipe-embedder` | Koyo | `chrome/browser/devtools/remote_debugging_server.cc` |
| `dialog-without-parent-titlebar` | Koyo | `ui/views/widget/native_widget_mac.mm` |
| `download-outlives-its-page` | Koyo | `components/download/` (`DownloadRequester`), `content/browser/download/download_manager_impl.{h,cc}`, `content/public/browser/download_manager_delegate.{h,cc}`, `content/browser/devtools/protocol/devtools_download_manager_delegate.{h,cc}`, `chrome/browser/download/chrome_download_manager_delegate.{h,cc}`, `cef/libcef/browser/download_manager_delegate_impl.{h,cc}`, `cef/include/cef_download_handler.h`, `cef/libcef_dll/{ctocpp/download_handler_ctocpp,cpptoc/download_handler_cpptoc}.cc`: who asked for a download travels with it, so the download handler of the browser that asked is told of it after that browser is gone, its browser then NULL, and its profile is kept until it ends (`docs/browser.md` 16.2) |
| `external-pump-wake-ups` | Koyo | `cef/libcef/browser/browser_message_loop.cc`: a turn of an external pump ends by asking for the wakeup Chromium's last task left it, as Chromium's own pumps do, so work left when the turn ran out of time and work put off from within a task ask for their turn, and the host's pump runs CEF's work only when it's asked (`docs/native-pages.md` 3.1). Its turn ends at `Quit()` and rounds the wait up, as Chromium's own pumps do. CEF has no such change, to 2026-11-20; its own example pump runs the work every thirtieth of a second instead (CEF issue 2968) |
| `extension-current-window-last-active` | Koyo | `chrome/browser/extensions/window_controller_list.cc` |
| `extension-install-ui-null-tab` | Cra | `chrome/browser/ui/extensions/extension_install_ui_desktop.cc` |
| `extension-popup-browser-info` | Cra | `cef/libcef/browser/browser_info_manager.cc` |
| `extension-tab-activate` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extension-tabs-create` | Cra | `cef/libcef/browser/chrome/`, `chrome/browser/extensions/api/tabs/tabs_api.cc` |
| `extensions-normal-windows` | Cra | `chrome/browser/extensions/api/extension_action/`, `chrome/browser/extensions/browser_extension_window_controller.cc` |
| `extensions-without-a-toolbar` | Koyo | `chrome/browser/ui/views/extensions/embedder_extensions_container.{h,cc}` (new) and `BUILD.gn`, `chrome/browser/ui/views/toolbar/toolbar_view.{h,cc}` and `BUILD.gn`, `cef/libcef/browser/chrome/` (`browser_delegate.h`, `chrome_browser_delegate.{h,cc}`, `chrome_browser_host_impl.cc`): a page the client marks for it (`koyo_extensions` in its `extra_info`, `HasEmbedderExtensions`), which has no toolbar of Chrome's (`ChromeToolbarType::NONE`), keeps its extensions' actions in a container of no views, held by the hidden toolbar Chrome gives a popup-type browser, so they run, their popups open and their shortcuts work; a popup nobody pressed for hangs from the top of the page. A popup's Browser has its popup's `extra_info`. A browser with no mark, a DevTools one, has no container. Carrying Chrome's whole toolbar hidden for its container laid it out on every frame (`docs/native-pages.md` 5) |
| `koyo-extensions-protocol` | Koyo, parts after Cra | the `Extensions` DevTools domain (`Extensions.pdl`, `extensions_handler.{h,cc}`, `inspector_protocol_config.json`), `cef/libcef/browser/chrome/browser_delegate.h`, `chrome_browser_delegate.cc` |
| `koyo-popup` | Koyo | the `Extensions` domain again (`koyoActionPointerDown`, the button `koyoTriggerAction` takes), `chrome/browser/ui/extensions/extension_popup_types.h`, `extension_action_delegate.h`, `chrome/browser/ui/views/extensions/extension_action_delegate_desktop.{h,cc}`, `extension_popup.{h,cc}`, `extension_view_views.{h,cc}`, `ui/views/bubble/bubble_dialog_delegate_view.{h,cc}` |
| `page-asked-before-close` | Koyo | `cef/libcef/browser/chrome/` (`chrome_browser_host_impl.{h,cc}`, `chrome_browser_web_contents_delegate.{h,cc}`), `cef/include/cef_browser.h`, `cef_life_span_handler.h`: `CloseBrowser(true)` closes a Chrome style page without asking it, as CEF documents, and a page `TryCloseBrowser` asked that may be left is the client's to close or keep (`DoClose`, CEF issue 3294) (`docs/browser.md` 17) |
| `request-outlives-its-page` | Koyo | `cef/libcef/browser/net_service/resource_request_handler_wrapper.cc`: a request the client wasn't offered outlives its browser, as Chrome's do; CEF aborted it, which ended a download as cancelled (`docs/browser.md` 16.2) |

- **Order matters where two touch a file.** `extensions-without-a-toolbar` goes over
  `extension-tab-activate` in CEF's browser delegate, `koyo-extensions-protocol` over
  `extension-tab-activate` and `extension-tabs-create` there too, and `koyo-popup` over
  `koyo-extensions-protocol` in the `Extensions` domain's files; the names sort that way.
- **Made against** CEF 158.0.6 (`23f1736`) on Chromium 158.0.8127.89. On another version
  each has to apply again: `script/cef` stops with the name of one that doesn't.
- **Cra's** are from `github.com/VictorKildahl/cra` (`scripts/cef-patches`), a macOS browser
  that embeds Chrome-style CEF pages as Koyo does, under the MIT licence below; their comments
  say "Cra" where they tell why. Changed here: the marker of an extension's tab is
  `koyo_extension_tab`, and the message that asks for a tab in front is `Koyo.activate`.
  The icon and badge part of Cra's `extensions-action-updates` is inside
  `koyo-extensions-protocol`, reworked. Its `extension-trigger-action-null-checks` is
  Chromium's own since 158: `triggerAction` answers with an error there.

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
