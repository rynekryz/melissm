import { navigate } from 'astro:transitions/client';
import { addToQueue, current, playQueue, playRadio, subscribe, type Track } from './player';
import { addToPlaylist, createPlaylist, getPlaylists, isLiked, toggleLike } from './library';

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

export type TrackOpts = { onUnlike?: () => void; onRemove?: (t: Track) => void; radio?: boolean };

const prevTrackIds = new WeakMap<HTMLElement, Set<string>>();
const prevCardIds = new WeakMap<HTMLElement, Set<string>>();

export function renderTracks(el: HTMLElement, tracks: Track[], opts: TrackOpts = {}) {
  if (!tracks.length) {
    prevTrackIds.set(el, new Set());
    el.innerHTML = '<p class="px-2 py-6 text-on-surface-variant">Nothing here yet.</p>';
    return;
  }
  const prev = prevTrackIds.get(el);
  prevTrackIds.set(el, new Set(tracks.map((t) => t.id)));
  let born = 0;
  el.innerHTML = tracks
    .map(
      (t, i) => {
        const fresh = !prev || !prev.has(t.id);
        const delay = fresh ? ` style="animation-delay:${Math.min(born++, 16) * 40}ms"` : '';
        return `<div class="track${fresh ? ' track-in' : ''}" data-i="${i}" data-id="${t.id}"${delay}>
        ${art(t, 'size-14 rounded-2xl shrink-0')}
        <div class="min-w-0 flex-1"><div class="truncate font-medium">${esc(t.title)}</div><div class="truncate text-sm opacity-70">${t.artist_id && !t.is_album ? `<span data-artist class="cursor-pointer py-2 hover:font-semibold">${esc(t.artist)}</span>` : esc(t.artist)}${t.album && !t.is_album ? ' • ' + esc(t.album) : ''}</div></div>
        <button data-next class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Play next"><span class="ms text-on-surface-variant">playlist_play</span></button>
        <button data-add class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Add to queue"><span class="ms text-on-surface-variant">playlist_add</span></button>
        <button data-pl class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Add to playlist"><span class="ms text-on-surface-variant">library_add</span></button>
        <button data-like class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Like"><span class="ms ${isLiked(t.id) ? 'fill text-primary' : ''}">favorite</span></button>
        <span class="w-12 text-right text-sm opacity-70">${t.duration ? fmt(t.duration) : ''}</span>
        ${opts.onRemove ? '<button data-remove class="press grid size-10 place-items-center rounded-full hover:bg-surface-container-highest" aria-label="Remove"><span class="ms text-on-surface-variant">delete</span></button>' : ''}
      </div>`;
      },
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
    const pl = target.closest<HTMLElement>('[data-pl]');
    if (pl) {
      e.stopPropagation();
      openPlaylistMenu(pl, track);
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
    if (opts.radio) playRadio(track);
    else playQueue(tracks, +row.dataset.i!);
  };
  markActive();
}

let plMenu: HTMLElement | null = null;
let plKey: ((e: KeyboardEvent) => void) | null = null;
const closeMenu = () => {
  plMenu?.remove();
  plMenu = null;
  if (plKey) {
    document.removeEventListener('keydown', plKey, true);
    plKey = null;
  }
};

export function confirmDialog(message: string): Promise<boolean> {
  return new Promise((resolve) => {
    const back = document.createElement('div');
    back.className = 'modal-back fixed inset-0 z-[70] grid place-items-center bg-black/50 p-6 backdrop-blur-lg';
    back.innerHTML = `<div class="modal-card w-full max-w-sm rounded-[28px] bg-surface-container-high p-6 shadow-2xl shadow-black/50">
      <p class="text-lg font-medium">${esc(message)}</p>
      <div class="mt-6 flex justify-end gap-2">
        <button data-no class="press h-12 rounded-full px-6 font-medium hover:bg-surface-container-highest">Cancel</button>
        <button data-yes class="press h-12 rounded-full px-6 font-medium text-error hover:bg-error/10">Delete</button>
      </div>
    </div>`;
    document.body.appendChild(back);
    const key = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      done(false);
    };
    const done = (v: boolean) => {
      document.removeEventListener('keydown', key, true);
      back.remove();
      resolve(v);
    };
    document.addEventListener('keydown', key, true);
    back.querySelector<HTMLElement>('[data-no]')!.focus();
    back.querySelector('[data-no]')!.addEventListener('click', () => done(false));
    back.querySelector('[data-yes]')!.addEventListener('click', () => done(true));
    back.addEventListener('click', (e) => {
      if (e.target === back) done(false);
    });
  });
}

export function promptDialog(message: string, value = '', confirmLabel = 'Save'): Promise<string | null> {
  return new Promise((resolve) => {
    const back = document.createElement('div');
    back.className = 'modal-back fixed inset-0 z-[70] grid place-items-center bg-black/50 p-6 backdrop-blur-lg';
    back.innerHTML = `<div class="modal-card w-full max-w-sm rounded-[28px] bg-surface-container-high p-6 shadow-2xl shadow-black/50">
      <p class="text-lg font-medium">${esc(message)}</p>
      <input data-field type="text" aria-label="${esc(message)}" autocomplete="off" spellcheck="false" value="${esc(value)}" class="mt-4 w-full rounded-2xl bg-surface-container-highest px-4 py-3 text-on-surface outline-none" />
      <div class="mt-6 flex justify-end gap-2">
        <button data-no class="press h-12 rounded-full px-6 font-medium hover:bg-surface-container-highest">Cancel</button>
        <button data-yes class="press h-12 rounded-full px-6 font-medium text-primary hover:bg-primary/10">${esc(confirmLabel)}</button>
      </div>
    </div>`;
    document.body.appendChild(back);
    const field = back.querySelector<HTMLInputElement>('[data-field]')!;
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        done(null);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        submit();
      }
    };
    const done = (v: string | null) => {
      document.removeEventListener('keydown', key, true);
      back.remove();
      resolve(v);
    };
    const submit = () => {
      const name = field.value.trim();
      done(name ? name : null);
    };
    document.addEventListener('keydown', key, true);
    back.querySelector('[data-no]')!.addEventListener('click', () => done(null));
    back.querySelector('[data-yes]')!.addEventListener('click', submit);
    back.addEventListener('click', (e) => {
      if (e.target === back) done(null);
    });
    field.focus();
    field.select();
  });
}

function openPlaylistMenu(anchor: HTMLElement, track: Track) {
  closeMenu();
  const lists = getPlaylists();
  plMenu = document.createElement('div');
  plMenu.className = 'plmenu';
  plMenu.innerHTML =
    lists.map((p) => `<button data-pid="${p.id}">${esc(p.name)}</button>`).join('') +
    '<button data-new><span class="ms">add</span>New playlist</button>';
  document.body.appendChild(plMenu);
  const r = anchor.getBoundingClientRect();
  const m = plMenu;
  m.style.top = `${Math.min(window.innerHeight - m.offsetHeight - 8, r.bottom + 6)}px`;
  m.style.left = `${Math.max(8, r.right - m.offsetWidth)}px`;
  plKey = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      closeMenu();
    }
  };
  document.addEventListener('keydown', plKey, true);
  m.querySelector<HTMLElement>('button')?.focus();
  m.addEventListener('click', async (e) => {
    e.stopPropagation();
    const b = (e.target as HTMLElement).closest<HTMLElement>('button');
    if (!b) return;
    closeMenu();
    if (b.dataset.new) {
      const name = await promptDialog('New playlist name', '', 'Create');
      if (name) {
        const id = await createPlaylist(name);
        if (id) await addToPlaylist(id, track);
      }
    } else {
      await addToPlaylist(+b.dataset.pid!, track);
    }
  });
  setTimeout(() => document.addEventListener('click', closeMenu, { once: true }), 0);
}

export function renderCards(el: HTMLElement, tracks: Track[], opts: { row?: boolean } = {}) {
  if (!tracks.length) {
    prevCardIds.set(el, new Set());
    el.innerHTML = '';
    return;
  }
  const prev = prevCardIds.get(el);
  prevCardIds.set(el, new Set(tracks.map((t) => t.id)));
  let born = 0;
  const size = opts.row ? 'w-[168px]' : '';
  el.innerHTML = tracks
    .map(
      (t, i) => {
        const fresh = !prev || !prev.has(t.id);
        const delay = fresh ? ` style="animation-delay:${Math.min(born++, 16) * 50}ms"` : '';
        return `<div class="card group${fresh ? ' card-in' : ''} ${size}" data-i="${i}" data-id="${t.id}"${delay}>
        <div class="relative">
          ${art(t, `card-art aspect-square w-full rounded-[32px] shadow-lg shadow-black/30`)}
          <span class="absolute bottom-2 right-2 grid size-10 place-items-center rounded-full bg-primary/90 text-on-primary opacity-0 transition-opacity duration-300 group-hover:opacity-100"><span class="ms fill">${t.is_album || t.is_playlist || t.is_artist ? 'arrow_forward' : 'play_arrow'}</span></span>
        </div>
        <div class="mt-3 truncate px-2 font-medium">${esc(t.title)}</div>
        <div class="truncate px-2 text-sm text-on-surface-variant">${t.artist_id && t.artist ? `<span data-artist class="cursor-pointer py-2 hover:font-semibold">${esc(t.artist)}</span>` : esc(t.artist || (t.is_album ? 'Album' : t.is_playlist ? 'Playlist' : t.is_artist ? 'Artist' : ''))}</div>
      </div>`;
      },
    )
    .join('');
  el.onclick = (e) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[data-i]');
    if (!row) return;
    const t = tracks[+row.dataset.i!];
    if ((e.target as HTMLElement).closest('[data-artist]') && t.artist_id) return navigate(`/artist?id=${t.artist_id}`);
    if (t.is_artist) return navigate(`/artist?id=${t.id}`);
    if (t.is_album || t.is_playlist) return navigate(`/album?id=${t.id}`);
    playQueue(tracks, +row.dataset.i!);
  };
}
