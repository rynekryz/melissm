import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauri, radio } from './api';
import { pushRecent } from './library';

export type Track = { id: string; title: string; artist: string; album: string; duration: number; thumbnail: string; is_album?: boolean };
export type RepeatMode = 'off' | 'all' | 'one';
export type Quality = 'max' | 'high' | 'low';
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
  quality: Quality;
};

const VOL_KEY = 'melissm.volume';
const SHUFFLE_KEY = 'melissm.shuffle';
const REPEAT_KEY = 'melissm.repeat';
const QUALITY_KEY = 'melissm.quality';

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
  quality: (['max', 'high', 'low'].includes(localStorage.getItem(QUALITY_KEY) ?? '')
    ? localStorage.getItem(QUALITY_KEY)
    : 'max') as Quality,
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
    album: t?.album ?? '',
    thumbnail: t?.thumbnail ?? '',
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
  holdUntil = 0;
  state.position = 0;
  state.duration = state.queue[index].duration;
  emit();
  try {
    state.duration = (await invoke<number>('play', { id: state.queue[index].id, quality: state.quality })) || state.duration;
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

let extending = false;
export async function extendQueue(): Promise<boolean> {
  const t = current();
  if (!t || extending) return false;
  extending = true;
  try {
    const related = await radio(t.id);
    const fresh = related.filter((r) => !state.queue.some((q) => q.id === r.id)).slice(0, 25);
    if (fresh.length) {
      state.queue.push(...fresh);
      emit();
      return true;
    }
    return false;
  } catch {
    return false;
  } finally {
    extending = false;
  }
}

export async function next() {
  if (state.shuffle && state.queue.length > 1) {
    let i = state.index;
    while (i === state.index) i = Math.floor(Math.random() * state.queue.length);
    return playAt(i);
  }
  if (state.index >= state.queue.length - 1) {
    const added = await extendQueue();
    if (!added) {
      state.playing = false;
      emit();
      return;
    }
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

export function setQuality(quality: Quality) {
  state.quality = quality;
  localStorage.setItem(QUALITY_KEY, quality);
  emit();
}

let holdPlaying = false;
let holdUntil = 0;
const holdPlayingTo = (playing: boolean) => {
  holdPlaying = playing;
  holdUntil = Date.now() + 700;
  state.playing = playing;
  emit();
};
const playingHeld = (reported: boolean) => {
  if (Date.now() < holdUntil && reported !== holdPlaying) return holdPlaying;
  holdUntil = 0;
  return reported;
};

export async function toggle() {
  if (!current() || state.loading) return;
  holdPlayingTo(!state.playing);
  await invoke(state.playing ? 'resume' : 'pause');
}

let seekHold = 0;
let seekTarget = 0;
export async function seek(seconds: number) {
  state.position = seconds;
  seekTarget = seconds;
  seekHold = Date.now();
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
    state.playing = playingHeld(e.payload.playing);
    if (seekHold && Date.now() - seekHold < 700 && Math.abs(e.payload.position - seekTarget) > 1.2) {
      state.position = seekTarget;
    } else {
      seekHold = 0;
      state.position = e.payload.position;
    }
    state.duration = e.payload.duration || state.duration;
    emit();
  });
  listen('track-ended', () => {
    if (state.repeat === 'one') return void playAt(state.index);
    if (state.index >= state.queue.length - 1 && state.repeat === 'all') return void playAt(0);
    void next();
  });  listen<string>('tray-action', (e) => {
    const p = e.payload;
    switch (p) {
      case 'play-pause': void toggle(); break;
      case 'play': if (current() && !state.playing && !state.loading) { holdPlayingTo(true); void invoke('resume'); } break;
      case 'pause': if (state.playing) { holdPlayingTo(false); void invoke('pause'); } break;
      case 'next': void next(); break;
      case 'previous': void previous(); break;
      case 'shuffle': toggleShuffle(); break;
      case 'repeat': cycleRepeat(); break;
      default:
        if (p.startsWith('seek:')) void seek(parseFloat(p.slice(5)));
        else if (p.startsWith('volume:')) void setVolume(parseFloat(p.slice(7)));
        break;
    }
  });
  listen<string>('playback-failed', (e) => {
    state.playing = false;
    state.loading = false;
    emit();
    bus.dispatchEvent(new CustomEvent('error', { detail: String(e.payload) }));
  });
  invoke('volume', { value: state.volume });
  const speed = parseFloat(localStorage.getItem('melissm.speed') ?? '1');
  const pitch = parseInt(localStorage.getItem('melissm.pitch') ?? '0', 10);
  void invoke('set_speed', { value: Number.isFinite(speed) ? speed : 1 });
  void invoke('set_pitch', { value: Number.isFinite(pitch) ? Math.pow(2, pitch / 12) : 1 });
  try {
    const eq = JSON.parse(localStorage.getItem('melissm.eq') ?? '[]');
    if (Array.isArray(eq) && eq.length === 10) eq.forEach((g: number, i: number) => void invoke('set_eq', { band: i, gain: +g }));
  } catch {}
  emit();
}
