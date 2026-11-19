import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getSettings, isTauri } from './api';

export type Track = {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
  thumbnail: string;
  is_album?: boolean;
  is_artist?: boolean;
  is_playlist?: boolean;
  artist_id?: string | null;
};
export type RepeatMode = 'off' | 'all' | 'one';
export type Quality = 'max' | 'high' | 'low';
export type PlayerState = {
  tracks: Track[];
  index: number;
  playing: boolean;
  loading: boolean;
  position: number;
  duration: number;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  quality: Quality;
  crossfade: number;
};

const state: PlayerState = {
  tracks: [],
  index: -1,
  playing: false,
  loading: false,
  position: 0,
  duration: 0,
  volume: 1,
  shuffle: false,
  repeat: 'off',
  quality: 'max',
  crossfade: 0,
};
const bus = new EventTarget();

export const current = (): Track | undefined => state.tracks[state.index];
export const getVolume = () => state.volume;
export const getQuality = () => state.quality;

export function subscribe(fn: (s: PlayerState) => void) {
  const handler = () => fn(state);
  bus.addEventListener('change', handler);
  handler();
  return () => bus.removeEventListener('change', handler);
}

export function playQueue(tracks: Track[], index: number, src?: string, cont?: string | null) {
  if (!isTauri) return;
  void invoke('play_tracks', { tracks, index, src: src ?? null, cont: cont ?? null });
}

export function playRadio(track: Track) {
  if (!isTauri) return;
  void invoke('play_radio', { track });
}

export function playAt(index: number) {
  if (!isTauri) return;
  void invoke('play_at', { index });
}

export const next = () => {
  if (isTauri) void invoke('skip_next');
};

export const previous = () => {
  if (isTauri) void invoke('skip_prev_cmd');
};

export function toggle() {
  if (!isTauri || state.loading) return;
  holdPlayingTo(!state.playing);
  void invoke('toggle_play_cmd');
}

export const toggleShuffle = () => {
  if (!isTauri) return;
  state.shuffle = !state.shuffle;
  emit();
  void invoke('toggle_shuffle_cmd').catch(() => {
    state.shuffle = !state.shuffle;
    emit();
  });
};
export const cycleRepeat = () => {
  if (!isTauri) return;
  state.repeat = state.repeat === 'off' ? 'all' : state.repeat === 'all' ? 'one' : 'off';
  emit();
  void invoke('cycle_repeat_cmd').catch(() => {
    state.repeat = state.repeat === 'off' ? 'all' : state.repeat === 'all' ? 'one' : 'off';
    emit();
  });
};

export function setQuality(quality: Quality) {
  state.quality = quality;
  if (isTauri) void invoke('set_quality', { quality });
}

export async function seek(seconds: number) {
  state.position = seconds;
  seekTarget = seconds;
  seekHold = Date.now();
  emit();
  if (isTauri) await invoke('seek', { seconds });
}

export function setVolume(value: number) {
  state.volume = value;
  userVolume = true;
  if (isTauri) void invoke('volume', { value });
}

export function setCrossfade(seconds: number) {
  state.crossfade = seconds;
  if (isTauri) void invoke('set_crossfade', { seconds });
}

export function addToQueue(track: Track, nextUp = false) {
  if (isTauri) void invoke('add_to_queue', { track, next: nextUp });
}

export function removeFromQueue(index: number) {
  if (isTauri) void invoke('remove_from_queue', { index });
}

export function moveInQueue(from: number, to: number) {
  if (isTauri) void invoke('move_in_queue', { from, to });
}

export function clearUpcoming() {
  if (isTauri) void invoke('clear_upcoming');
}

export function onError(fn: (msg: string) => void) {
  bus.addEventListener('error', (e) => fn(String((e as CustomEvent).detail)));
}

const emit = () => bus.dispatchEvent(new Event('change'));

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

let seekHold = 0;
let seekTarget = 0;
let queueSeq = 0;
let userVolume = false;

if (isTauri) {
  listen<{
    tracks: Track[];
    index: number;
    shuffle: boolean;
    repeat: RepeatMode;
    loading: boolean;
  }>('queue-state', (e) => {
    queueSeq++;
    state.tracks = e.payload.tracks;
    state.index = e.payload.index;
    state.shuffle = e.payload.shuffle;
    state.repeat = e.payload.repeat;
    state.loading = e.payload.loading;
    emit();
  });
  listen<{ playing: boolean; position: number; duration: number }>('player-state', (e) => {
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
  listen<string>('playback-failed', (e) => {
    state.playing = false;
    state.loading = false;
    emit();
    bus.dispatchEvent(new CustomEvent('error', { detail: String(e.payload) }));
  });
  void (async () => {
    const seq = queueSeq;
    try {
      const queue = await invoke<{ tracks: Track[]; index: number; shuffle: boolean; repeat: RepeatMode; loading: boolean }>(
        'get_queue',
      );
      if (seq === queueSeq) {
        state.tracks = queue.tracks;
        state.index = queue.index;
        state.loading = queue.loading;
        state.shuffle = queue.shuffle;
        state.repeat = queue.repeat;
      }
      const settings = await getSettings();
      if (!userVolume) {
        const vol = parseFloat(settings.volume ?? '1');
        state.volume = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 1;
      }
      if (['max', 'high', 'low'].includes(settings.quality)) state.quality = settings.quality as Quality;
      const fade = parseInt(settings.crossfade ?? '0', 10);
      state.crossfade = Number.isFinite(fade) ? fade : 0;
      emit();
    } catch {}
  })();
}
