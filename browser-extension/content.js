// Runs in real contest tab: source fetches use logged-in page session.
(() => {
  if (window.top !== window || window.__cptuiSolutionsInstalled) return;
  window.__cptuiSolutionsInstalled = true;

  const core = globalThis.CptuiSolutionsCore;
  let running = false;
  let controller;
  let lastApi = 0;
  let panel;

  chrome.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message?.type !== 'cptui-fetch-solutions') return;
    if (running) { reply({ ok: false, error: 'already running' }); return; }
    start(message.serverUrl).catch(() => {});
    reply({ ok: true });
  });

  function show(text, error = false) {
    if (!panel) {
      panel = document.createElement('div');
      panel.style.cssText = [
        'position:fixed', 'right:16px', 'bottom:16px', 'z-index:2147483647',
        'max-width:360px', 'padding:12px 14px', 'border-radius:8px',
        'background:#1f2937', 'color:#f9fafb', 'font:13px system-ui,sans-serif',
        'box-shadow:0 4px 20px #0008', 'white-space:pre-wrap',
      ].join(';');
      const cancel = document.createElement('button');
      cancel.textContent = 'Cancel';
      cancel.style.cssText = 'float:right;margin-left:12px;cursor:pointer';
      cancel.onclick = cancelRun;
      panel.append(cancel, document.createElement('span'));
      document.body.append(panel);
    }
    panel.querySelector('span').textContent = text;
    panel.style.background = error ? '#7f1d1d' : '#1f2937';
  }

  function hide() {
    panel?.remove();
    panel = undefined;
  }

  function cancelRun() {
    controller?.abort(); // abort every in-flight page/API/local-server fetch
    hide(); // immediate: never leave an inert “Cancelling…” overlay behind
  }

  function wireProblem(problem) {
    return {
      platform: problem.platform,
      contest_id: String(problem.contestId),
      index: problem.index,
      title: problem.title || 'problem',
    };
  }

  function progress(server, problem, stage, completed, total, message, signal) {
    void fetch(server + '/solutions/progress', {
      method: 'POST', signal, headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ problem: wireProblem(problem), stage, completed, total, message }),
    }).catch(() => {});
  }

  async function start(rawServer) {
    if (running) return;
    const problem = core.parseProblem(location.href);
    if (!problem) { show('cptui: open a Codeforces or AtCoder problem page', true); return; }
    const server = String(rawServer || 'http://127.0.0.1:27121').replace(/\/+$/, '');
    running = true;
    controller = new AbortController();
    const { signal } = controller;
    const target = problem.platform === 'VJudge' ? 3 : 30;
    problem.title = core.pageTitle(problem.platform);
    try {
      let files;
      if (problem.platform === 'AtCoder') files = await collectAtCoder(problem, server, signal);
      else if (problem.platform === 'VJudge') files = await collectVJudge(problem, server, signal);
      else files = await collectCodeforces(problem, server, signal);
      if (!files.length) {
        if (problem.platform === 'VJudge') {
          throw new Error('VJudge returned no source text. Open leaderboard rows may expose only source images; text requires each author’s share-token URL.');
        }
        throw new Error('No source pages were available. Log into the contest site and reload this tab once.');
      }

      show(`cptui: saving ${files.length} sources…`);
      progress(server, problem, 'Saving', files.length, target, 'Sending source batch to cptui', signal);
      const response = await fetch(server + '/solutions', {
        method: 'POST', signal, headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ problem: wireProblem(problem), files }),
      });
      if (!response.ok) {
        const detail = (await response.text()).replace(/\s+/g, ' ').slice(0, 240);
        throw new Error(`cptui receiver HTTP ${response.status}${detail ? `: ${detail}` : ''}`);
      }
      show(`cptui: done — ${files.length} sources saved.`);
      progress(server, problem, 'Complete', files.length, target, `${files.length} source files sent`, signal);
    } catch (error) {
      if (signal.aborted || error?.name === 'AbortError') return;
      const message = error?.message || String(error);
      show(`cptui: ${message}`, true);
      progress(server, problem, 'Failed', 0, 30, message, signal);
    } finally {
      running = false;
      controller = undefined;
    }
  }

  async function collectCodeforces(problem, server, signal) {
    show('cptui: loading Codeforces rated users…');
    progress(server, problem, 'Discovering', 0, 30, 'Loading rated users', signal);
    const rated = await cfTopRated(signal);
    const handles = new Set(rated.map((u) => u.handle.toLowerCase()));
    const accepted = new Map();
    const pageSize = 10000;
    for (let from = 1, page = 1; accepted.size < 60 && page <= 6; page++, from += pageSize) {
      show(`cptui: scanning Codeforces submissions (${page}/6)…`);
      progress(server, problem, 'Scanning', page, 6, `Scanning submission page ${page}`, signal);
      const subs = await cfApi('contest.status', { contestId: problem.contestId, from, count: pageSize }, signal);
      for (const [handle, submission] of core.acceptedFrom(subs, problem.index, handles)) {
        const old = accepted.get(handle);
        if (!old || old.id < submission.id) accepted.set(handle, submission);
      }
      if (subs.length < pageSize) break;
    }
    const candidates = core.selectTop(accepted, rated, 60);
    if (!candidates.length) throw new Error('No accepted submissions from top-rated users found.');
    return fetchSourceBatches(problem, server, candidates, signal, async (c) => {
      const response = await fetch(`/problemset/submission/${problem.contestId}/${c.id}`, { credentials: 'include', signal });
      return response.ok ? core.sourceFrom(await response.text()) : null;
    });
  }

  async function collectAtCoder(problem, server, signal) {
    show('cptui: ranking AtCoder accepted users…');
    progress(server, problem, 'Discovering', 0, 30, 'Ranking contest-time accepted users by pre-contest rating', signal);
    const standingsResponse = await fetch(`/contests/${problem.contestId}/standings/json`, { credentials: 'include', signal });
    const standings = await standingsResponse.json().catch(() => null);
    if (!standings?.StandingsData) {
      throw new Error('Could not read AtCoder standings. Log into AtCoder and reload this tab once.');
    }
    const ranked = core.atCoderRatedUsers(standings, problem.index, 60);
    if (!ranked.length) throw new Error('No rated contest participants accepted this task.');

    const candidates = [];
    for (let offset = 0; offset < ranked.length && candidates.length < 30; offset += 6) {
      const users = ranked.slice(offset, offset + 6);
      show(`cptui: locating AtCoder sources ${candidates.length}/30…`);
      progress(server, problem, 'Scanning', candidates.length, 30, 'Locating accepted submissions for ranked users', signal);
      const rows = await Promise.all(users.map(async (user) => {
        const query = new URLSearchParams({ 'f.Task': problem.index, 'f.Status': 'AC', 'f.User': user.handle });
        const response = await fetch(`/contests/${problem.contestId}/submissions?${query}`, { credentials: 'include', signal });
        return core.atCoderRows(await response.text())[0] || null;
      }));
      rows.forEach((row) => { if (row && candidates.length < 30) candidates.push(row); });
    }
    if (!candidates.length) throw new Error('Could not locate accepted submissions for ranked AtCoder users.');
    return fetchSourceBatches(problem, server, candidates, signal, async (c) => {
      const response = await fetch(`/contests/${problem.contestId}/submissions/${c.id}`, { credentials: 'include', signal });
      return response.ok ? core.sourceFrom(await response.text()) : null;
    });
  }

  function mainWorldFetch(request, signal) {
    return new Promise((resolve, reject) => {
      if (signal.aborted) return reject(new DOMException('Cancelled', 'AbortError'));
      const onAbort = () => reject(new DOMException('Cancelled', 'AbortError'));
      signal.addEventListener('abort', onAbort, { once: true });
      chrome.runtime.sendMessage({ type: 'cptui-vjudge-main-fetch', request }, (reply) => {
        signal.removeEventListener('abort', onAbort);
        if (signal.aborted) return;
        if (chrome.runtime.lastError) return reject(new Error(chrome.runtime.lastError.message));
        if (reply?.error) return reject(new Error(reply.error));
        resolve(reply?.result);
      });
    });
  }

  async function collectVJudge(problem, server, signal) {
    show('cptui: reading VJudge leaderboard…');
    progress(server, problem, 'Discovering', 0, 30, 'Reading ranked open submissions', signal);
    const query = new URLSearchParams({
      draw: '1', start: '0', length: '20', sortDir: 'asc', sortCol: '2', language: '', _: String(Date.now()),
    });
    const response = await mainWorldFetch({
      url: `/problem/leaderBoard/${problem.index}?${query}`,
      headers: {
        Accept: 'application/json, text/javascript, */*; q=0.01',
        'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
        'X-Requested-With': 'XMLHttpRequest',
      },
    }, signal);
    const text = response?.text || '';
    if (!response?.ok) {
      const detail = text.replace(/\s+/g, ' ').slice(0, 240);
      throw new Error(`VJudge leaderboard HTTP ${response?.status || 'request failure'}${detail ? `: ${detail}` : ''}`);
    }
    const body = (() => { try { return JSON.parse(text); } catch { return null; } })();
    if (!body?.data) throw new Error('VJudge leaderboard returned non-JSON. Log into VJudge and reload this tab once.');
    const candidates = core.vjudgeCandidates(body?.data, 3);
    if (!candidates.length) throw new Error('No open VJudge sources in this leaderboard.');
    let lastError = '';
    const files = await fetchSourceBatches(problem, server, candidates, signal, async (c) => {
      const source = await mainWorldFetch({
        url: `/solution/data/${c.id}`,
        method: 'POST',
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
          'X-Requested-With': 'XMLHttpRequest',
        },
        body: 'shareCode=',
      }, signal);
      try {
        if (!source?.ok) {
          lastError = `solution/data HTTP ${source?.status}: ${(source?.text || '').replace(/\s+/g, ' ').slice(0, 200)}`;
          return null;
        }
        const data = JSON.parse(source?.text || '{}');
        const code = core.vjudgeSource(data);
        if (code) return { source: code };
        const imageUrl = core.vjudgeImage(data);
        if (!imageUrl) {
          lastError = `solution ${c.id}: no code, no image. keys=[${Object.keys(data).slice(0, 12).join(',')}]`;
          return null;
        }
        const image = await mainWorldFetch({ url: imageUrl, binary: true }, signal);
        if (image?.ok && image.base64) {
          return { source: '', imageBase64: image.base64, imageMime: image.contentType };
        }
        lastError = `source image HTTP ${image?.status || 'request failure'} for ${c.id}`;
        return null;
      } catch (e) {
        lastError = `solution ${c.id}: ${e?.message || 'parse failed'}`;
        return null;
      }
    }, 3);
    if (!files.length && lastError) throw new Error(`VJudge ${lastError}`);
    return files;
  }

  async function fetchSourceBatches(problem, server, candidates, signal, fetchOne, target = 30) {
    const files = [];
    for (let offset = 0; offset < candidates.length && files.length < target; offset += 6) {
      const chunk = candidates.slice(offset, offset + 6);
      show(`cptui: fetching sources ${files.length}/${target}…`);
      progress(server, problem, 'Fetching', files.length, target, `Fetching ${chunk.length} sources in parallel`, signal);
      const sources = await Promise.all(chunk.map(fetchOne));
      chunk.forEach((candidate, i) => {
        if (sources[i] && files.length < target) {
          const payload = typeof sources[i] === 'string' ? { source: sources[i] } : sources[i];
          files.push({
            handle: candidate.handle,
            submission_id: Number(candidate.id),
            ext: core.ext(candidate.language),
            source: payload.source || '',
            image_base64: payload.imageBase64,
            image_mime: payload.imageMime,
          });
        }
      });
    }
    return files;
  }

  async function cfTopRated(signal) {
    const { ratedCache } = await chrome.storage.local.get('ratedCache');
    if (ratedCache?.at && Date.now() - ratedCache.at < 86400000 && ratedCache.users?.length) return ratedCache.users;
    const users = await cfApi('user.ratedList', { activeOnly: 'false' }, signal);
    const top = users.filter((u) => typeof u.rating === 'number').sort((a, b) => b.rating - a.rating)
      .slice(0, 500).map((u) => ({ handle: u.handle, rating: u.rating }));
    await chrome.storage.local.set({ ratedCache: { at: Date.now(), users: top } });
    return top;
  }

  async function cfApi(method, params, signal) {
    const wait = lastApi + 1100 - Date.now();
    if (wait > 0) await abortableSleep(wait, signal);
    lastApi = Date.now();
    const query = new URLSearchParams();
    Object.entries(params).forEach(([k, v]) => query.set(k, String(v)));
    const response = await fetch(`/api/${method}?${query}`, { signal });
    const data = await response.json().catch(() => null);
    if (!data || data.status !== 'OK') throw new Error(`Codeforces API ${method}: ${data?.comment || response.status}`);
    return data.result;
  }

  function abortableSleep(ms, signal) {
    return new Promise((resolve, reject) => {
      const id = setTimeout(resolve, ms);
      signal.addEventListener('abort', () => {
        clearTimeout(id);
        reject(new DOMException('Cancelled', 'AbortError'));
      }, { once: true });
    });
  }
})();