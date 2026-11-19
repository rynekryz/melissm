import { invoke } from '@tauri-apps/api/core';
import {
  historyList,
  isTauri,
  likesList,
  playlistAdd,
  playlistCreate,
  playlistDelete,
  playlistRemove,
  playlistRename,
  playlistTracks,
  playlistsList,
  storeImport,
} from './api';
import type { Track } from './player';
import type { PlaylistMeta } from './types';

let likes: Track[] = [];
let history: Track[] = [];
let playlists: PlaylistMeta[] = [];
const bus = new EventTarget();

const emit = () => bus.dispatchEvent(new Event('change'));

export function subscribe(fn: () => void) {
  const handler = () => fn();
  bus.addEventListener('change', handler);
  return () => bus.removeEventListener('change', handler);
}

let initPromise: Promise<void> | null = null;

export function initLibrary(): Promise<void> {
  if (!isTauri) return Promise.resolve();
  if (initPromise) return initPromise;
  initPromise = (async () => {
    likes = await likesList();
    history = await historyList();
    playlists = await playlistsList();
    if (!likes.length && !history.length) {
      const read = (key: string): Track[] => {
        try {
          return JSON.parse(localStorage.getItem(key) ?? '[]');
        } catch {
          return [];
        }
      };
      const oldLikes = read('melissm.likes');
      const oldRecent = read('melissm.recent');
      if (oldLikes.length || oldRecent.length) {
        await storeImport(oldLikes, oldRecent);
        localStorage.removeItem('melissm.likes');
        localStorage.removeItem('melissm.recent');
        likes = await likesList();
        history = await historyList();
      }
    }
    emit();
  })().catch((e) => {
    initPromise = null;
    throw e;
  });
  return initPromise;
}

export const getLikes = () => likes;
export const getHistory = () => history;
export const getPlaylists = () => playlists;
export const isLiked = (id: string) => likes.some((t) => t.id === id);

export async function refreshHistory() {
  if (!isTauri) return;
  history = await historyList();
  emit();
}

export async function toggleLike(track: Track): Promise<boolean> {
  if (!isTauri) return false;
  const liked = await invoke<boolean>('like_toggle', { track });
  likes = await likesList();
  emit();
  return liked;
}

export function clearLikes() {
  if (!isTauri) return;
  void invoke('likes_clear');
  likes = [];
  emit();
}

export function removeHistory(id: string) {
  if (!isTauri) return;
  void invoke('history_remove', { id });
  history = history.filter((t) => t.id !== id);
  emit();
}

export function clearHistory() {
  if (!isTauri) return;
  void invoke('history_clear');
  history = [];
  emit();
}

export async function createPlaylist(name: string): Promise<number | null> {
  if (!isTauri || !name.trim()) return null;
  const id = await playlistCreate(name.trim());
  playlists = await playlistsList();
  emit();
  return id;
}

export async function renamePlaylist(id: number, name: string) {
  if (!isTauri || !name.trim()) return;
  await playlistRename(id, name.trim());
  playlists = await playlistsList();
  emit();
}

export async function deletePlaylist(id: number) {
  if (!isTauri) return;
  await playlistDelete(id);
  playlists = await playlistsList();
  emit();
}

export const fetchPlaylistTracks = (id: number) => playlistTracks(id);

export async function addToPlaylist(id: number, track: Track, afterId?: string | null) {
  if (!isTauri) return;
  await playlistAdd(id, track, afterId ?? null);
  playlists = await playlistsList();
  emit();
}

export async function removeFromPlaylist(id: number, trackId: string) {
  if (!isTauri) return;
  await playlistRemove(id, trackId);
  playlists = await playlistsList();
  emit();
}
