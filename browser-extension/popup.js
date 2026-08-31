const server = document.getElementById('server');
const status = document.getElementById('status');

document.getElementById('start').onclick = async () => {
  const serverUrl = server.value.trim().replace(/\/+$/, '') || 'http://127.0.0.1:27121';
  await chrome.storage.local.set({ cptuiServerUrl: serverUrl });
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  try {
    await chrome.tabs.sendMessage(tab.id, { type: 'cptui-fetch-solutions', serverUrl });
    status.textContent = 'Started. See status overlay on Codeforces page and cptui footer.';
  } catch {
    // First click after install: inject scripts into already-open tab, then retry.
    try {
      await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ['core.js', 'content.js'] });
      await chrome.tabs.sendMessage(tab.id, { type: 'cptui-fetch-solutions', serverUrl });
      status.textContent = 'Started. See status overlay on Codeforces page and cptui footer.';
    } catch (e) {
      status.textContent = 'Open/reload a Codeforces problem first: ' + e.message;
    }
  }
};

chrome.storage.local.get('cptuiServerUrl').then(({ cptuiServerUrl }) => {
  if (cptuiServerUrl) server.value = cptuiServerUrl;
});