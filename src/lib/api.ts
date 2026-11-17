import { invoke } from '@tauri-apps/api/core';
import type { Track } from './player';
import type { AlbumPage, ArtistPage, PlaylistMeta, Section } from './types';

export type KWord = { t: string; s: number };
export type KLine = { text: string; start: number; words: KWord[] | null };
export type Lyrics = { synced: string | null; plain: string | null; karaoke: KLine[] | null; provider: string; custom: boolean };
export type ThemeMode = 'auto' | 'dark' | 'light';
export type ThemeSource = { wallpaper: null } | { artwork: null } | { color: string };

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const search = (query: string) => invoke<Track[]>('search', { query });
export const playlistsForYou = (query: string) => invoke<Track[]>('playlists_for_you', { query });
export const home = () => invoke<Section[]>('home');
export const album = (id: string) => invoke<AlbumPage>('album', { id });
export const albumMore = (token: string) => invoke<[Track[], string | null]>('album_more', { token });
export const setAutoload = (enabled: boolean) => invoke('set_autoload', { enabled });
export const artist = (id: string) => invoke<ArtistPage>('artist', { id });
export const lyrics = (t: Track) =>
  invoke<Lyrics>('lyrics', { id: t.id, title: t.title, artist: t.artist, album: t.album, duration: t.duration });
export const lyricsGetCustom = (id: string) => invoke<string | null>('lyrics_get_custom', { id });
export const lyricsSaveCustom = (id: string, text: string) => invoke('lyrics_save_custom', { id, text });
export const lyricsDeleteCustom = (id: string) => invoke('lyrics_delete_custom', { id });
export const lyricsSetProvider = (provider: string, enabled: boolean) => invoke('lyrics_set_provider', { provider, enabled });
export const lyricsSetOffset = (offset: number) => invoke('lyrics_set_offset', { offset });
export const setTheme = (theme: { mode: ThemeMode; scheme: string; source: ThemeSource }) =>
  invoke<Record<string, string>>('set_theme', { theme });
export const systemScale = () => invoke<number>('system_scale');
export const setSpeed = (value: number) => invoke('set_speed', { value });
export const setPitch = (value: number) => invoke('set_pitch', { value });
export const setEq = (band: number, gain: number) => invoke('set_eq', { band, gain });
export const setCrossfade = (seconds: number) => invoke('set_crossfade', { seconds });
export const getSettings = () => invoke<Record<string, string>>('get_settings');
export const cacheInfo = () => invoke<number>('cache_info');
export const cacheClear = () => invoke<number>('cache_clear');
export const setAutocache = (enabled: boolean) => invoke('set_autocache', { enabled });
export const likesList = () => invoke<Track[]>('likes_list');
export const historyList = () => invoke<Track[]>('history_list');
export const playlistsList = () => invoke<PlaylistMeta[]>('playlists_list');
export const playlistCreate = (name: string) => invoke<number>('playlist_create', { name });
export const playlistRename = (id: number, name: string) => invoke('playlist_rename', { id, name });
export const playlistDelete = (id: number) => invoke('playlist_delete', { id });
export const playlistTracks = (id: number) => invoke<Track[]>('playlist_tracks', { id });
export const playlistAdd = (id: number, track: Track, afterId?: string | null) =>
  invoke('playlist_add', { id, track, afterId: afterId ?? null });
export const playlistRemove = (id: number, trackId: string) => invoke('playlist_remove', { id, trackId });
export const storeImport = (likes: Track[], history: Track[]) => invoke('store_import', { likes, history });
