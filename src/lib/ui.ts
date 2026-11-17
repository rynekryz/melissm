import { navigate } from 'astro:transitions/client';
import { addToQueue, current, playQueue, subscribe, type Track } from './player';
import { isLiked, toggleLike } from './library';

export const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;

export const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);

const art = (t: Track, cls: string) =>
  t.thumbnail
    ? `<img src="${esc(t.thumbnail)}" alt="" loading="lazy" class="${cls} object-cover bg-surface-container-highest">`
    : `<div class="${cls} grid place-items-center bg-surface-container-highest"><span class="ms text-on-surface-variant opacity-60">${t.is_album || t.is_playlist ? 'album' : 'music_note'}</span></div>`;

const markActive = () => {
  const id = current()?.id;
  document.querySelectorAll<HTMLElement>('[data-id]').forEach((el) => el.classList.toggle('is-active', el.dataset.id === id));
};
if (typeof document !== 'undefined') subscribe(markActive);

export function renderTracks(el: HTMLElement, tracks: Track[], opts: { onUnlike?: () => void; onRemove?: (t: Track) => void } = {}) {
  if (!tracks.length) {
    el.innerHTML = '<p class="px-2 py-6 text-on-surface-variant">Nothing here yet.</p>';
    return;
  }
  el.innerHTML = tracks
    .map(
      (t, i) => `<div class="track" data-i="${i}" data-id="${t.id}">
        ${art(t, 'size-14 rounded-2xl shrink-0')}
        <div class="min-w-0 flex-1"><div class="truncate font-medium">${esc(t.title)}</div><div class="truncate text-sm opacity-70">${t.artist_id && !t.is_album ? `<span data-artist class="cursor-pointer hover:underline">${esc(t.artist)}</span>` : esc(t.artist)}${t.album && !t.is_album ? ' • ' + esc(t.album) : ''}</div></div>
        <button data-next class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Play next"><span class="ms text-on-surface-variant">playlist_play</span></button>
        <button data-add class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Add to queue"><span class="ms text-on-surface-variant">playlist_add</span></button>
        <button data-like class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Like"><span class="ms ${isLiked(t.id) ? 'fill text-primary' : ''}">favorite</span></button>
        <span class="w-12 text-right text-sm opacity-70">${t.duration ? fmt(t.duration) : ''}</span>
        ${opts.onRemove ? '<button data-remove class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Remove"><span class="ms text-on-surface-variant">delete</span></button>' : ''}
      </div>`,
    )
    .join('');
  el.onclick = (e) => {
    const target = e.target as HTMLElement;
    const row = target.closest<HTMLElement>('[data-i]');
    if (!row) return;
    const track = tracks[+row.dataset.i!];
    const artist = target.closest<HTMLElement>('[data-artist]');
    if (artist && track.artist_id) {
      navigate(`/artist?id=${track.artist_id}`);
      return;
    }
    const nextUp = target.closest('[data-next]');
    if (nextUp) {
      addToQueue(track, true);
      return;
    }
    const add = target.closest('[data-add]');
    if (add) {
      addToQueue(track, false);
      return;
    }
    const like = target.closest('[data-like]');
    if (like) {
      void toggleLike(track).then((liked) => {
        like.firstElementChild!.className = `ms ${liked ? 'fill text-primary' : ''}`;
        if (!liked) opts.onUnlike?.();
      });
      return;
    }
    const remove = target.closest('[data-remove]');
    if (remove) {
      opts.onRemove?.(track);
      return;
    }
    playQueue(tracks, +row.dataset.i!);
  };
  markActive();
}

export function renderCards(el: HTMLElement, tracks: Track[], opts: { row?: boolean } = {}) {
  if (!tracks.length) {
    el.innerHTML = '';
    return;
  }
  const size = opts.row ? 'w-[168px]' : '';
  el.innerHTML = tracks
    .map(
      (t, i) => `<div class="card group ${size}" data-i="${i}" data-id="${t.id}">
        <div class="relative">
          ${art(t, `card-art aspect-square w-full rounded-[32px] shadow-lg shadow-black/30`)}
          <span class="absolute bottom-2 right-2 grid size-10 place-items-center rounded-full bg-primary/90 text-on-primary opacity-0 transition-opacity duration-300 group-hover:opacity-100"><span class="ms fill">${t.is_album || t.is_playlist || t.is_artist ? 'arrow_forward' : 'play_arrow'}</span></span>
        </div>
        <div class="mt-3 truncate px-2 font-medium">${esc(t.title)}</div>
        <div class="truncate px-2 text-sm text-on-surface-variant">${esc(t.artist || (t.is_album ? 'Album' : t.is_playlist ? 'Playlist' : t.is_artist ? 'Artist' : ''))}</div>
      </div>`,
    )
    .join('');
  el.onclick = (e) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[data-i]');
    if (!row) return;
    const t = tracks[+row.dataset.i!];
    if (t.is_artist) return navigate(`/artist?id=${t.id}`);
    if (t.is_album || t.is_playlist) return navigate(`/album?id=${t.id}`);
    playQueue(tracks, +row.dataset.i!);
  };
}
