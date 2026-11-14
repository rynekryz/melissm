import { invoke } from '@tauri-apps/api/core';
import type { Track } from './player';
import type { Section } from './types';

export type Lyrics = { synced: string | null; plain: string | null };
export type ThemeMode = 'auto' | 'dark' | 'light';
export type ThemeSource = { wallpaper: null } | { color: string };

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const search = (query: string) => invoke<Track[]>('search', { query });
export const home = () => invoke<Section[]>('home');
export const album = (id: string) => invoke<Track[]>('album', { id });
export const lyrics = (t: Track) =>
  invoke<Lyrics>('lyrics', { title: t.title, artist: t.artist, album: t.album, duration: t.duration });
export const setTheme = (theme: { mode: ThemeMode; scheme: string; source: ThemeSource }) =>
  invoke<Record<string, string>>('set_theme', { theme });
export const systemScale = () => invoke<number>('system_scale');
export const radio = (id: string) => invoke<Track[]>('radio', { id });
export const setSpeed = (value: number) => invoke('set_speed', { value });
export const setPitch = (value: number) => invoke('set_pitch', { value });
export const setEq = (band: number, gain: number) => invoke('set_eq', { band, gain });
