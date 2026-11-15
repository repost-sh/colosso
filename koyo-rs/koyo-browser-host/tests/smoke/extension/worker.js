// The extension the browser host's smoke test loads (tests/smoke/extensions.rs). It sets a
// badge for every tab as it starts, and at a press on its icon says what it was pressed on
// and which tab it's told is the current one, then opens a tab; pressed again, it closes
// that tab.
chrome.action.setBadgeText({ text: "7" });

let opened = null;

chrome.action.onClicked.addListener(async (tab) => {
  // The second press closes the tab the first one opened.
  if (opened !== null) {
    await chrome.tabs.remove(opened);
    opened = null;
    return;
  }
  await chrome.action.setBadgeText({ text: "ok", tabId: tab.id });
  const [current] = await chrome.tabs.query({ active: true, currentWindow: true });
  const windows = await chrome.windows.getAll();
  const said = { pressed: tab.url, current: current ? current.url : "", window: windows[0].type };
  await chrome.action.setTitle({ title: JSON.stringify(said) });
  const made = await chrome.tabs.create({ url: new URL("/opened", tab.url).href, active: false });
  opened = made.id;
  await chrome.action.setBadgeText({ text: made.id ? "tab" : "none", tabId: tab.id });
});
