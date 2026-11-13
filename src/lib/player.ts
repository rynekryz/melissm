import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from './api';
import { pushRecent } from './library';

export type Track = { id: string; title: string; artist: string; album: string; duration: number; thumbnail: string; is_album?: boolean };
export type RepeatMode = 'off' | 'all' | 'one';
export type PlayerState = {
  queue: Track[];
  index: number;
  playing: boolean;
  loading: boolean;
  position: number;
  duration: number;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
};

const VOL_KEY = 'melissm.volume';
const SHUFFLE_KEY = 'melissm.shuffle';
const REPEAT_KEY = 'melissm.repeat';

const state: PlayerState = {
  queue: [],
  index: -1,
  playing: false,
  loading: false,
  position: 0,
  duration: 0,
  volume: (() => {
    const v = parseFloat(localStorage.getItem(VOL_KEY) ?? '1');
    return Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 1;
  })(),
  shuffle: localStorage.getItem(SHUFFLE_KEY) === '1',
  repeat: (['off', 'all', 'one'].includes(localStorage.getItem(REPEAT_KEY) ?? '')
    ? localStorage.getItem(REPEAT_KEY)
    : 'off') as RepeatMode,
};
const bus = new EventTarget();
let lastSync = '';
const syncTray = () => {
  const sig = JSON.stringify([state.playing, state.index, state.shuffle, state.repeat, current()?.id]);
  if (sig === lastSync) return;
  lastSync = sig;
  const t = current();
  void invoke('sync_player_state', {
    playing: state.playing,
    shuffle: state.shuffle,
    repeat: state.repeat,
    title: t?.title ?? '',
    artist: t?.artist ?? '',
  }).catch(() => {});
};
const emit = () => {
  syncTray();
  bus.dispatchEvent(new Event('change'));
};

export const current = (): Track | undefined => state.queue[state.index];

export function subscribe(fn: (s: PlayerState) => void) {
  const handler = () => fn(state);
  bus.addEventListener('change', handler);
  handler();
  return () => bus.removeEventListener('change', handler);
}

export async function playAt(index: number) {
  if (index < 0 || index >= state.queue.length) return;
  state.index = index;
  state.loading = true;
  state.playing = false;
  state.position = 0;
  state.duration = state.queue[index].duration;
  emit();
  try {
    state.duration = (await invoke<number>('play', { id: state.queue[index].id })) || state.duration;
    pushRecent(state.queue[index]);
  } catch (e) {
    if (String(e) === 'superseded') return;
    state.loading = false;
    state.playing = false;
    emit();
    bus.dispatchEvent(new CustomEvent('error', { detail: String(e) }));
    return;
  }
  state.loading = false;
  emit();
}

export async function playQueue(tracks: Track[], index: number) {
  state.queue = tracks;
  await playAt(index);
}

export async function next() {
  if (state.shuffle && state.queue.length > 1) {
    let i = state.index;
    while (i === state.index) i = Math.floor(Math.random() * state.queue.length);
    return playAt(i);
  }
  return playAt(state.index + 1);
}
export const previous = () => (state.position > 4 ? seek(0) : playAt(state.index - 1));

export function toggleShuffle() {
  state.shuffle = !state.shuffle;
  localStorage.setItem(SHUFFLE_KEY, state.shuffle ? '1' : '0');
  emit();
}

export function cycleRepeat() {
  state.repeat = state.repeat === 'off' ? 'all' : state.repeat === 'all' ? 'one' : 'off';
  localStorage.setItem(REPEAT_KEY, state.repeat);
  emit();
}

export async function toggle() {
  if (!current() || state.loading) return;
  await invoke(state.playing ? 'pause' : 'resume');
}

export async function seek(seconds: number) {
  state.position = seconds;
  emit();
  await invoke('seek', { seconds });
}

export async function setVolume(value: number) {
  state.volume = value;
  localStorage.setItem(VOL_KEY, String(value));
  await invoke('volume', { value });
}

export function onError(fn: (msg: string) => void) {
  bus.addEventListener('error', (e) => fn(String((e as CustomEvent).detail)));
}

if (isTauri) {
  listen<{ playing: boolean; position: number; duration: number }>('player-state', (e) => {
    if (state.loading) return;
    state.playing = e.payload.playing;
    state.position = e.payload.position;
    state.duration = e.payload.duration || state.duration;
    emit();
  });
  listen('track-ended', () => {
    if (state.repeat === 'one') return void playAt(state.index);
    if (state.index >= state.queue.length - 1 && state.repeat === 'all') return void playAt(0);
    void next();
  });
  listen<string>('tray-action', (e) => {
    switch (e.payload) {
      case 'play-pause': void toggle(); break;
      case 'next': void next(); break;
      case 'previous': void previous(); break;
      case 'shuffle': toggleShuffle(); break;
      case 'repeat': cycleRepeat(); break;
    }
  });
  listen<string>('playback-failed', (e) => {
    state.playing = false;
    state.loading = false;
    emit();
    bus.dispatchEvent(new CustomEvent('error', { detail: String(e.payload) }));
  });
  invoke('volume', { value: state.volume });
  emit();
}
