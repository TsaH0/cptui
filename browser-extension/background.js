// VJudge rejects isolated-world fetches. Execute its AJAX calls in page main world.
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== 'cptui-vjudge-main-fetch') return;
  const tabId = sender.tab?.id;
  if (!tabId) { sendResponse({ error: 'no browser tab' }); return; }
  chrome.scripting.executeScript({
    target: { tabId },
    world: 'MAIN',
    args: [message.request],
    func: async (request) => new Promise((resolve) => {
      // VJudge's backend differentiates XHR from fetch initiated by extensions.
      const xhr = new XMLHttpRequest();
      xhr.open(request.method || 'GET', request.url, true);
      xhr.withCredentials = true;
      if (request.binary) xhr.responseType = 'arraybuffer';
      Object.entries(request.headers || {}).forEach(([key, value]) => xhr.setRequestHeader(key, value));
      xhr.onload = () => {
        const base = { ok: xhr.status >= 200 && xhr.status < 300, status: xhr.status };
        if (!request.binary) return resolve({ ...base, text: xhr.responseText });
        const bytes = new Uint8Array(xhr.response || new ArrayBuffer(0));
        let raw = '';
        for (const byte of bytes) raw += String.fromCharCode(byte);
        resolve({ ...base, base64: btoa(raw), contentType: xhr.getResponseHeader('content-type') || 'image/png' });
      };
      xhr.onerror = () => resolve({ ok: false, status: xhr.status || 0, text: xhr.responseText || '' });
      xhr.send(request.body || null);
    }),
  }).then((results) => sendResponse({ result: results[0]?.result }))
    .catch((error) => sendResponse({ error: error.message }));
  return true;
});