import { describe, expect, test } from 'vitest';
import { canAdd, canStart, initialState, reduce, totals, waitingCount, type Action, type State } from './state';
import type { EntryState, QueueEntry, QueueEvent } from './types';

function entry(id: number, opts: Partial<QueueEntry> = {}): QueueEntry {
  return { id, group: null, url: `u${id}`, title: `T${id}`, prefix: null, state: { type: 'waiting' }, ...opts };
}

function added(entries: QueueEntry[], group: { id: number; title: string } | null = null): Action {
  return { type: 'queueEvent', event: { type: 'added', group, entries } };
}

function ev(event: QueueEvent): Action {
  return { type: 'queueEvent', event };
}

function run(actions: Action[], from: State = initialState): State {
  return actions.reduce(reduce, from);
}

const ready: State = { ...initialState, folder: 'C:\\Videos' };

describe('link box', () => {
  test('add is ignored while a link is being read', () => {
    const s = run([
      { type: 'linkSubmitted', url: 'u1' },
      { type: 'linkSubmitted', url: 'u2' },
    ]);
    expect(s.link).toBe('reading');
    expect(s.linkUrl).toBe('u1');
  });

  test('blank submit clears the box', () => {
    const s = run([{ type: 'linkSubmitted', url: '   ' }]);
    expect(s.link).toBe('idle');
    expect(s.linkUrl).toBe('');
  });

  test('stale link result is ignored', () => {
    const cleared = run([{ type: 'linkSubmitted', url: 'u1' }, { type: 'linkCleared' }]);
    expect(cleared.link).toBe('idle');
    expect(reduce(cleared, { type: 'linkRead', url: 'u1' })).toEqual(cleared);
    expect(reduce(cleared, { type: 'linkFailed', url: 'u1', message: 'x' })).toEqual(cleared);
    // A result for an older url while a newer one is being read is also ignored.
    const second = run([{ type: 'linkSubmitted', url: 'u2' }]);
    expect(reduce(second, { type: 'linkRead', url: 'u1' })).toEqual(second);
  });

  test('link read clears the box', () => {
    const s = run([
      { type: 'linkSubmitted', url: 'u1' },
      { type: 'linkRead', url: 'u1' },
    ]);
    expect(s.link).toBe('idle');
    expect(s.linkUrl).toBe('');
  });

  test('link failure shows the message and keeps the box', () => {
    const s = run([
      { type: 'linkSubmitted', url: 'u1' },
      { type: 'linkFailed', url: 'u1', message: 'Not a video' },
    ]);
    expect(s.link).toBe('idle');
    expect(s.linkUrl).toBe('u1');
    expect(s.linkError).toBe('Not a video');
  });

  test('canAdd needs an idle box and no missing tools', () => {
    expect(canAdd(initialState)).toBe(true);
    expect(canAdd(run([{ type: 'linkSubmitted', url: 'u' }]))).toBe(false);
    expect(canAdd(run([{ type: 'toolsChecked', missing: ['yt-dlp'] }]))).toBe(false);
  });
});

describe('queue events', () => {
  test('queue events build and update rows', () => {
    let s = run([added([entry(1), entry(2)])]);
    expect(waitingCount(s)).toBe(2);
    expect(s.rows[0].percent).toBeNull();

    s = run(
      [
        ev({ type: 'status', status: 'running' }),
        ev({ type: 'state', id: 1, state: { type: 'running' } }),
        ev({ type: 'progress', id: 1, done: 40, total: 100 }),
      ],
      s,
    );
    expect(s.status).toBe('running');
    expect(s.rows[0].state).toEqual({ type: 'running' });
    expect(s.rows[0].percent).toBe(40);
    expect(waitingCount(s)).toBe(1);

    s = run([ev({ type: 'phase', id: 1, phase: 'merging' })], s);
    expect(s.rows[0].phase).toBe('merging');
    expect(s.rows[0].percent).toBe(0);

    s = run([ev({ type: 'progress', id: 1, done: 5, total: null })], s);
    expect(s.rows[0].percent).toBeNull();

    s = run([ev({ type: 'state', id: 1, state: { type: 'done', path: 'C:\\Videos\\a.mp4' } })], s);
    expect(s.lastDonePath).toBe('C:\\Videos\\a.mp4');
    expect(s.rows[0].percent).toBeNull();
    expect(s.rows[1].state).toEqual({ type: 'waiting' });
  });

  test('events for unknown ids are harmless', () => {
    const s = run([added([entry(1)])]);
    expect(run([ev({ type: 'progress', id: 9, done: 1, total: 2 })], s).rows).toEqual(s.rows);
  });

  test('added appends rows and the group', () => {
    const s = run([added([entry(1)]), added([entry(2, { group: 7 }), entry(3, { group: 7 })], { id: 7, title: 'List' })]);
    expect(s.rows.map((r) => r.id)).toEqual([1, 2, 3]);
    expect(s.groups).toEqual([{ id: 7, title: 'List' }]);
  });

  test('removed drops rows and the group', () => {
    let s = run([added([entry(1), entry(2, { group: 7 }), entry(3, { group: 7 })], { id: 7, title: 'List' })]);
    s = run([ev({ type: 'removed', ids: [2, 3], group: 7 })], s);
    expect(s.rows.map((r) => r.id)).toEqual([1]);
    expect(s.groups).toEqual([]);
  });

  test('several removed events in a row apply independently', () => {
    let s = run([
      added([entry(1)]),
      added([entry(2, { group: 7 })], { id: 7, title: 'A' }),
      added([entry(3, { group: 8 }), entry(4, { group: 8 })], { id: 8, title: 'B' }),
    ]);
    s = run(
      [
        ev({ type: 'removed', ids: [1], group: null }),
        ev({ type: 'removed', ids: [2], group: 7 }),
        ev({ type: 'removed', ids: [3], group: null }),
      ],
      s,
    );
    expect(s.rows.map((r) => r.id)).toEqual([4]);
    expect(s.groups).toEqual([{ id: 8, title: 'B' }]);
  });

  test('failure toggle opens and closes, and removal closes it', () => {
    const failed: EntryState = { type: 'failed', msg: 'boom' };
    let s = run([added([entry(1)]), ev({ type: 'state', id: 1, state: failed }), { type: 'failureToggled', id: 1 }]);
    expect(s.openFailure).toBe(1);
    expect(run([{ type: 'failureToggled', id: 1 }], s).openFailure).toBeUndefined();
    s = run([ev({ type: 'removed', ids: [1], group: null })], s);
    expect(s.openFailure).toBeUndefined();
  });
});

describe('derived', () => {
  test('canStart', () => {
    const withWaiting = run([added([entry(1)])], ready);
    expect(canStart(withWaiting)).toBe(true);
    expect(canStart(ready)).toBe(false); // nothing waiting
    expect(canStart({ ...withWaiting, folder: '' })).toBe(false);
    expect(canStart({ ...withWaiting, missingTools: ['ffmpeg'] })).toBe(false);
    expect(canStart({ ...withWaiting, status: 'running' })).toBe(false);
    expect(canStart({ ...withWaiting, status: 'paused' })).toBe(true);
  });

  test('totals count done, failed and waiting', () => {
    const s = run([
      added([entry(1), entry(2), entry(3), entry(4)]),
      ev({ type: 'state', id: 1, state: { type: 'done', path: 'p' } }),
      ev({ type: 'state', id: 2, state: { type: 'failed', msg: 'm' } }),
      ev({ type: 'state', id: 3, state: { type: 'running' } }),
    ]);
    expect(totals(s)).toEqual({ done: 1, failed: 1, waiting: 1 });
  });

  test('choice change is stored', () => {
    const s = run([{ type: 'choiceChanged', choice: { type: 'mp3' } }]);
    expect(s.choice).toEqual({ type: 'mp3' });
    expect(initialState.choice).toEqual({ type: 'video', quality: { type: 'best' } });
  });
});

describe('playlist question', () => {
  const info = { title: 'V', durationSecs: null, thumbnailUrl: null, videoOptions: [], hasAudio: true };
  const ask: Action = { type: 'linkAsk', url: 'u1', info, lookup: { type: 'mix' } };
  const reading = (): State => run([{ type: 'linkSubmitted', url: 'u1' }]);

  test('linkAsk sets ask and frees the link box', () => {
    const s = reduce(reading(), ask);
    expect(s.ask).toEqual({ url: 'u1', info, lookup: { type: 'mix' } });
    expect(s.link).toBe('idle');
    expect(canAdd(s)).toBe(true);
  });

  test('stale linkAsk is ignored', () => {
    const cleared = run([{ type: 'linkSubmitted', url: 'u1' }, { type: 'linkCleared' }]);
    expect(reduce(cleared, ask).ask).toBeUndefined();
    const other = run([{ type: 'linkSubmitted', url: 'u2' }]);
    expect(reduce(other, ask).ask).toBeUndefined();
  });

  test('clearing removes the ask', () => {
    const s = run([ask, { type: 'linkCleared' }], reading());
    expect(s.ask).toBeUndefined();
    expect(s.linkUrl).toBe('');
  });

  test('a new submit replaces the ask', () => {
    const s = run([ask, { type: 'linkSubmitted', url: 'u2' }], reading());
    expect(s.ask).toBeUndefined();
    expect(s.link).toBe('reading');
    expect(s.linkUrl).toBe('u2');
  });

  test('linkRead after an ask clears it', () => {
    const s = run([ask, { type: 'linkRead', url: 'u1' }], reading());
    expect(s.ask).toBeUndefined();
    expect(s.linkUrl).toBe('');
  });

  test('linkFailed after an ask shows the error and drops the ask', () => {
    const s = run([ask, { type: 'linkFailed', url: 'u1', message: 'x' }], reading());
    expect(s.ask).toBeUndefined();
    expect(s.linkError).toBe('x');
  });
});

describe('remembered video quality', () => {
  test('MP3 then Video restores the last quality', () => {
    const s = run([
      { type: 'choiceChanged', choice: { type: 'video', quality: { type: 'upTo', height: 720 } } },
      { type: 'kindChanged', kind: 'mp3' },
      { type: 'kindChanged', kind: 'video' },
    ]);
    expect(s.choice).toEqual({ type: 'video', quality: { type: 'upTo', height: 720 } });
  });

  test('loaded quality is used when starting on MP3', () => {
    const s = run([
      { type: 'choiceChanged', choice: { type: 'mp3' } },
      { type: 'videoQualityLoaded', quality: { type: 'smallest' } },
      { type: 'kindChanged', kind: 'video' },
    ]);
    expect(s.choice).toEqual({ type: 'video', quality: { type: 'smallest' } });
  });

  test('choosing MP3 does not forget the quality', () => {
    const s = run([
      { type: 'choiceChanged', choice: { type: 'video', quality: { type: 'smallest' } } },
      { type: 'choiceChanged', choice: { type: 'mp3' } },
    ]);
    expect(s.videoQuality).toEqual({ type: 'smallest' });
  });
});
