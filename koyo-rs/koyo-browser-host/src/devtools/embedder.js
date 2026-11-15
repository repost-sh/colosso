// The DevTools frontend's embedder shim (docs/browser.md 13.2). The host evaluates it in the
// frontend's own page, over that page's own DevTools session, each time the page has loaded
// (embedder.rs, host/inspect.rs). Chromium attaches its own embedder to a frontend at a valid
// address (DevToolsUIBindings, reached through devtools_compatibility.js's DevToolsAPI), but
// for a page that isn't one of Chrome's DevTools windows its delegate does nothing: docking
// elsewhere and closing go nowhere. Every InspectorFrontendHost method, the × button's bound
// one too, sends through DevToolsAPI.sendMessageToEmbedder, so that is where the shim takes:
//
// - setIsDocked: the frontend has written the new side to its preferences just before; the
//   host is told the side, and the frontend is called back at once, as the embedder would.
// - closeWindow: the host is told.
// - setPreference of emulation.show-device-mode: the host is told, and it's passed on.
//
// The frontend boots after the page's load, docking itself where its preferences say and
// showing its device toolbar if they say so: none of that is the person asking, so the host
// is told of a side or the device mode only once the frontend is up and the shim has set it
// as Koyo has it (or, in a frontend it can't find its way round, once it has given up
// trying, long after the boot). That is the rest, which waits until then: the frontend is put at the side
// Koyo has the tools at, through its own setting (currentDockState), and its device toolbar
// is turned off, as Koyo's responsive mode is off for a page until turned on. Docked, the frontend shows a
// placeholder for the page beside itself, where Chrome puts the page; Koyo's page is in a pane
// of its own, so the placeholder is hidden now and after each docked layout (its
// AdvancedApp's updateForDocked, then its split's hideMain), and the tools fill their pane.
//
// Called as `(side, binding)`: the side Koyo has the tools at, and the name of the function
// the host added to the page (Runtime.addBinding) that tells the host. What it says is one of
// HostEvent::DevTools's asks, as JSON. It keeps what it did in globalThis.koyoDevTools, and
// evaluated again in the same page it does nothing. It returns what became of it.
((side, binding) => {
  'use strict';
  const FRONTEND = 'devtools://devtools/bundled/devtools_app.html';
  const where = globalThis.location;
  if (!where || !String(where.href).startsWith(FRONTEND)) return 'elsewhere';
  const STATE = 'koyoDevTools';
  if (globalThis[STATE]) return 'again';
  const api = globalThis.DevToolsAPI;
  const send = globalThis[binding];
  if (!api || typeof api.sendMessageToEmbedder !== 'function' || typeof send !== 'function') {
    return 'no embedder';
  }
  const { defineProperty } = Object;
  const { apply } = Reflect;
  const { stringify, parse } = JSON;
  const SIDES = ['bottom', 'left', 'right', 'undocked'];
  const DOCK = 'currentDockState';
  const DEVICE = 'emulation.show-device-mode';
  const opening = SIDES.includes(side) ? side : 'bottom';
  // What the shim did, for whoever looks: the side the frontend last wrote, whether its asks
  // are passed on yet (it's up and set as Koyo has it, or never got there: `unsettled`), and
  // how its page placeholder last was.
  const state = { side: opening, ready: false, unsettled: false, placeholder: 'shown' };
  defineProperty(globalThis, STATE, { value: state, configurable: true });

  const tell = (said) => {
    try {
      send(stringify(said));
    } catch {
      // The host isn't told; the frontend goes on as it would have.
    }
  };
  // What the frontend does to its side and device mode before it's up and set as Koyo has
  // it, as it boots and as the shim sets it, isn't the person asking.
  const asked = (said) => {
    if (state.ready) tell(said);
  };
  const read = (value) => {
    try {
      return parse(value);
    } catch {
      return undefined;
    }
  };

  const forward = api.sendMessageToEmbedder;
  api.sendMessageToEmbedder = function koyoSendMessageToEmbedder(method, args, callback) {
    const said = Array.isArray(args) ? args : [];
    if (method === 'closeWindow') {
      tell({ op: 'close' });
      return undefined;
    }
    if (method === 'setIsDocked') {
      const at = said[0] ? state.side : 'undocked';
      if (!said[0] || at !== 'undocked') asked({ op: 'dock', side: at });
      if (typeof callback === 'function') setTimeout(callback, 0);
      return undefined;
    }
    if (method === 'setPreference' && said[0] === DOCK) {
      const written = read(said[1]);
      if (SIDES.includes(written)) state.side = written;
    } else if (method === 'setPreference' && said[0] === DEVICE) {
      asked({ op: 'deviceMode', on: read(said[1]) === true });
    }
    return apply(forward, this, [method, args, callback]);
  };

  const hide = (app) => {
    const split = app.rootSplitWidget;
    if (split && typeof split.hideMain === 'function') split.hideMain();
    if (split && typeof split.showMode === 'function') state.placeholder = split.showMode();
  };
  const setting = (settings, name, fallback) => {
    try {
      return settings.moduleSetting(name);
    } catch {
      return fallback === undefined ? null : settings.createSetting(name, fallback);
    }
  };

  // Once the frontend is up: its AdvancedApp laid out, its settings made.
  const settle = (tries) => {
    let app = null;
    try {
      app = globalThis.Emulation.AdvancedApp.instance();
    } catch {
      // Not up yet.
    }
    const settings = globalThis.universe && globalThis.universe.settings;
    const docking = settings ? setting(settings, DOCK) : null;
    if (!app || !app.rootSplitWidget || !docking) {
      if (tries > 0) {
        setTimeout(() => settle(tries - 1), 50);
      } else {
        // A frontend this shim doesn't know its way round: its boot is long over, so what it
        // asks from here on is the person's, and the host is told, though the frontend was
        // never set as Koyo has it and its page placeholder shows.
        state.unsettled = true;
        state.ready = true;
      }
      return;
    }
    const { updateForDocked } = app;
    if (typeof updateForDocked === 'function') {
      app.updateForDocked = function updateForDockedAlone(...laid) {
        const done = apply(updateForDocked, this, laid);
        hide(this);
        return done;
      };
    }
    if (docking.get() !== opening) docking.set(opening);
    state.side = docking.get();
    const device = setting(settings, DEVICE, false);
    if (device && device.get()) device.set(false);
    hide(app);
    state.ready = true;
  };
  // Up to 20 s for the frontend to boot.
  settle(400);
  return 'installed';
})
