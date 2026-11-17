import type { Track } from './player';

export type Section = { title: string; tracks: Track[] };
export type AlbumPage = { title: string; subtitle: string; thumbnail: string; tracks: Track[]; next: string | null };
export type ArtistPage = { name: string; thumbnail: string; top: Track[]; albums: Track[]; singles: Track[] };
export type PlaylistMeta = { id: number; name: string; count: number };
