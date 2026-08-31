import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const sandbox = { URL };
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(new URL('../core.js', import.meta.url), 'utf8'), sandbox);
const core = sandbox.CptuiSolutionsCore;

test('parses Codeforces, AtCoder, and rejects gyms', () => {
  assert.equal(JSON.stringify(core.parseProblem('https://codeforces.com/contest/4/problem/A')), JSON.stringify({ platform: 'Codeforces', contestId: '4', index: 'A' }));
  assert.equal(JSON.stringify(core.parseProblem('https://www.codeforces.com/problemset/problem/1899/F1')), JSON.stringify({ platform: 'Codeforces', contestId: '1899', index: 'F1' }));
  assert.equal(JSON.stringify(core.parseProblem('https://atcoder.jp/contests/abc300/tasks/abc300_a')), JSON.stringify({ platform: 'AtCoder', contestId: 'abc300', index: 'abc300_a' }));
  assert.equal(JSON.stringify(core.parseProblem('https://vjudge.net/problem/DMOJ-hkccc08j2')), JSON.stringify({ platform: 'VJudge', contestId: 'vjudge', index: 'DMOJ-hkccc08j2' }));
  assert.equal(core.parseProblem('https://codeforces.com/gym/1/problem/A'), null);
});

test('keeps newest solo accepted submission from selected handles', () => {
  const handles = new Set(['benq']);
  const got = core.acceptedFrom([
    { id: 2, verdict: 'OK', problem: { index: 'A' }, author: { members: [{ handle: 'Benq' }] }, programmingLanguage: 'GNU C++20' },
    { id: 4, verdict: 'OK', problem: { index: 'A' }, author: { members: [{ handle: 'BENQ' }] }, programmingLanguage: 'Rust' },
    { id: 8, verdict: 'OK', problem: { index: 'A' }, author: { members: [{ handle: 'x' }] }, programmingLanguage: 'Python' },
    { id: 9, verdict: 'OK', problem: { index: 'A' }, author: { members: [{ handle: 'a' }, { handle: 'b' }] }, programmingLanguage: 'C++' },
  ], 'A', handles);
  assert.equal(JSON.stringify(got.get('benq')), JSON.stringify({ id: 4, language: 'Rust' }));
  assert.equal(got.size, 1);
});

test('selects rating order, maps language, and extracts source', () => {
  const candidates = core.selectTop(new Map([['b', { id: 1, language: 'GNU C++17' }]]), [
    { handle: 'a', rating: 3900 }, { handle: 'b', rating: 3500 },
  ], 30);
  assert.equal(JSON.stringify(candidates), JSON.stringify([{ handle: 'b', rating: 3500, id: 1, language: 'GNU C++17' }]));
  assert.equal(core.ext('PyPy 3-64'), 'py');
  assert.equal(core.ext('GNU C++20'), 'cpp');
  assert.equal(core.ext('unknown'), 'txt');
  assert.equal(core.sourceFrom('<pre id="submission-code">print(&quot;ok&quot;)</pre>'), 'print("ok")');
  assert.equal(core.vjudgeSource({ code: 'puts 1' }), 'puts 1');
  assert.equal(core.vjudgeImage({ codeImgUrl: '/image.png' }), '/image.png');
  assert.equal(core.vjudgeImage({}), null);
  assert.equal(core.vjudgeSource({}), null);
  assert.equal(JSON.stringify(core.atCoderRatedUsers({ StandingsData: [
    { UserScreenName: 'unrated', OldRating: 0, TaskResults: { abc300_a: { Score: 100, Status: 1 } } },
    { UserScreenName: 'skip', OldRating: 4000, TaskResults: { abc300_a: { Score: 0, Status: 0 } } },
    { UserScreenName: 'high', OldRating: 3500, TaskResults: { abc300_a: { Score: 100, Status: 1 } } },
    { UserScreenName: 'mid', OldRating: 2800, TaskResults: { abc300_a: { Score: 100, Status: 1 } } },
  ] }, 'abc300_a', 2)), JSON.stringify([
    { handle: 'high', rating: 3500 }, { handle: 'mid', rating: 2800 },
  ]));
  assert.equal(JSON.stringify(core.vjudgeCandidates([
    { rank: 1, isOpen: false, runId: 1, username: 'closed' },
    { rank: 2, isOpen: true, runId: 2, username: 'first', language: 'Python 3' },
    { rank: 3, isOpen: false, runId: 3, username: 'closed2' },
    { rank: 4, isOpen: true, runId: 4, username: 'second', languageCanonical: 'CPP' },
    { rank: 5, isOpen: true, runId: 5, username: 'third', language: 'Rust' },
    { rank: 6, isOpen: true, runId: 6, username: 'fourth', language: 'Java' },
  ])), JSON.stringify([
    { id: 2, handle: 'first', language: 'Python 3' },
    { id: 4, handle: 'second', language: 'CPP' },
    { id: 5, handle: 'third', language: 'Rust' },
  ]));
});