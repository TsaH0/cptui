// Shared pure helpers. Global so classic content script + popup-injected files work.
globalThis.CptuiSolutionsCore = (() => {
  function parseProblem(url) {
    let u;
    try { u = new URL(url); } catch { return null; }
    const p = u.pathname.split('/').filter(Boolean);
    if (/(^|\.)codeforces\.com$/.test(u.hostname)) {
      const goodIndex = (v) => /^[A-Za-z][0-9]{0,2}$/.test(v);
      if (p.length === 4 && p[0] === 'contest' && /^\d+$/.test(p[1]) && p[2] === 'problem' && goodIndex(p[3])) {
        return { platform: 'Codeforces', contestId: p[1], index: p[3].toUpperCase() };
      }
      if (p.length === 4 && p[0] === 'problemset' && p[1] === 'problem' && /^\d+$/.test(p[2]) && goodIndex(p[3])) {
        return { platform: 'Codeforces', contestId: p[2], index: p[3].toUpperCase() };
      }
    }
    if (/(^|\.)atcoder\.jp$/.test(u.hostname)
        && p.length === 4 && p[0] === 'contests' && p[2] === 'tasks' && /^[a-z0-9_]+$/i.test(p[3])) {
      return { platform: 'AtCoder', contestId: p[1], index: p[3] };
    }
    if (/(^|\.)vjudge\.net$/.test(u.hostname) && p.length === 2 && p[0] === 'problem' && p[1]) {
      return { platform: 'VJudge', contestId: 'vjudge', index: p[1] };
    }
    return null;
  }

  function acceptedFrom(submissions, index, handles) {
    const out = new Map();
    for (const s of submissions || []) {
      if (s.verdict !== 'OK' || s.problem?.index !== index) continue;
      const members = s.author?.members || [];
      if (members.length !== 1) continue;
      const handle = members[0].handle;
      const key = handle.toLowerCase();
      if (!handles.has(key)) continue;
      if (!out.has(key) || out.get(key).id < s.id) {
        out.set(key, { id: s.id, language: s.programmingLanguage });
      }
    }
    return out;
  }

  function selectTop(accepted, rated, count) {
    const out = [];
    for (const user of rated) {
      const s = accepted.get(user.handle.toLowerCase());
      if (s) out.push({ ...user, ...s });
      if (out.length >= count) break;
    }
    return out;
  }

  function ext(language) {
    const l = (language || '').toLowerCase();
    const rules = [
      ['c++', 'cpp'], ['g++', 'cpp'], ['clang', 'cpp'], ['c#', 'cs'], ['.net', 'cs'],
      ['python', 'py'], ['pypy', 'py'], ['java', 'java'], ['kotlin', 'kt'], ['rust', 'rs'],
      ['go', 'go'], ['ruby', 'rb'], ['haskell', 'hs'], ['pascal', 'pas'], ['perl', 'pl'],
      ['php', 'php'], ['javascript', 'js'], ['node', 'js'], ['typescript', 'ts'], ['scala', 'scala'],
      ['gnu c', 'c'],
    ];
    return rules.find(([needle]) => l.includes(needle))?.[1] || 'txt';
  }

  function decode(s) {
    return s.replace(/&(#[0-9]+|#x[0-9a-fA-F]+|[a-zA-Z]+);/g, (m, e) => {
      if (e[0] === '#') {
        const n = e[1] === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
        return Number.isFinite(n) && n > 0 && n < 0x10ffff ? String.fromCodePoint(n) : m;
      }
      return ({ lt: '<', gt: '>', amp: '&', quot: '"', apos: "'", nbsp: ' ' })[e] || m;
    });
  }

  function sourceFrom(html) {
    const m = html?.match(/<pre[^>]*id="(?:program-source-text|submission-code)"[^>]*>([\s\S]*?)<\/pre>/);
    return m ? decode(m[1]) : null;
  }

  function vjudgeSource(data) {
    return typeof data?.code === 'string' ? data.code : null;
  }

  function pageTitle(platform) {
    const selector = platform === 'AtCoder'
      ? '#task-statement span.h2, #task-statement h3'
      : '.problem-statement .header .title';
    const raw = document.querySelector(selector)?.textContent?.trim() || '';
    return raw.replace(/^[A-Za-z][A-Za-z0-9_]*\s*[-.]\s*/, '') || 'problem';
  }

  function atCoderRows(html) {
    const doc = new DOMParser().parseFromString(html, 'text/html');
    return [...doc.querySelectorAll('tr')].flatMap((row) => {
      const link = row.querySelector('a[href*="/submissions/"]');
      const m = link?.getAttribute('href')?.match(/\/submissions\/(\d+)/);
      if (!m) return [];
      const handle = row.querySelector('a[href^="/users/"]')?.textContent?.trim() || 'atcoder';
      return [{ id: m[1], handle, language: row.cells[3]?.textContent?.trim() || '' }];
    });
  }

  function vjudgeImage(data) {
    return typeof data?.codeImgUrl === 'string' ? data.codeImgUrl : null;
  }

  function atCoderRatedUsers(standings, task, count = 60) {
    const rating = (row) => Number(row.OldRating ?? row.Rating ?? 0) || 0;
    return (standings?.StandingsData || [])
      .filter((row) => {
        const result = row.TaskResults?.[task];
        return result && (result.Score > 0 || result.Status === 1);
      })
      .sort((a, b) => rating(b) - rating(a) || String(a.UserScreenName).localeCompare(String(b.UserScreenName)))
      .slice(0, count)
      .map((row) => ({ handle: row.UserScreenName, rating: rating(row) }));
  }

  function vjudgeCandidates(rows, count = 3) {
    return (rows || []).filter((row) => row.isOpen && row.runId).slice(0, count).map((row) => ({
      id: row.runId, handle: row.username, language: row.language || row.languageCanonical || '',
    }));
  }

  return { parseProblem, acceptedFrom, selectTop, ext, sourceFrom, vjudgeSource, vjudgeImage, atCoderRatedUsers, vjudgeCandidates, pageTitle, atCoderRows };
})();