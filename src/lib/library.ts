import type { Track } from './player';

const LIKES = 'melissm.likes';
const RECENT = 'melissm.recent';

const read = (key: string): Track[] => {
  try {
    return JSON.parse(localStorage.getItem(key) ?? '[]');
  } catch {
    return [];
  }
};

export const likes = (): Track[] => read(LIKES);
export const recent = (): Track[] => read(RECENT).slice(0, 50);

export const isLiked = (id: string) => likes().some((t) => t.id === id);

export function toggleLike(track: Track): boolean {
  const all = likes();
  const next = isLiked(track.id) ? all.filter((t) => t.id !== track.id) : [track, ...all];
  localStorage.setItem(LIKES, JSON.stringify(next));
  return isLiked(track.id);
}

export function pushRecent(track: Track) {
  if (track.is_album) return;
  const rest = read(RECENT).filter((t) => t.id !== track.id);
  localStorage.setItem(RECENT, JSON.stringify([track, ...rest].slice(0, 50)));
}

export function removeRecent(id: string) {
  localStorage.setItem(RECENT, JSON.stringify(read(RECENT).filter((t) => t.id !== id)));
}

export function clearLikes() {
  localStorage.removeItem(LIKES);
}

export function clearRecent() {
  localStorage.removeItem(RECENT);
}
