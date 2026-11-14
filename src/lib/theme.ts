import { listen } from '@tauri-apps/api/event';
import { isTauri, setTheme, systemScale, type ThemeMode, type ThemeSource } from './api';

export type ThemeOpts = {
  mode: ThemeMode;
  scheme: string;
  source: ThemeSource;
  blur: boolean;
  scale: 'dynamic' | number;
};

export const schemes = [
  { id: 'tonal-spot', label: 'Tonal spot' },
  { id: 'vibrant', label: 'Vibrant' },
  { id: 'expressive', label: 'Expressive' },
  { id: 'fidelity', label: 'Fidelity' },
  { id: 'content', label: 'Content' },
  { id: 'neutral', label: 'Neutral' },
  { id: 'monochrome', label: 'Monochrome' },
  { id: 'rainbow', label: 'Rainbow' },
  { id: 'fruit-salad', label: 'Fruit salad' },
];

const KEY = 'melissm.theme';

const defaults: ThemeOpts = { mode: 'auto', scheme: 'tonal-spot', source: { wallpaper: null }, blur: true, scale: 'dynamic' };

export const loadOpts = (): ThemeOpts => {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return {
      mode: raw.mode === 'dark' || raw.mode === 'light' ? raw.mode : 'auto',
      scheme: typeof raw.scheme === 'string' ? raw.scheme : 'tonal-spot',
      source: raw.source && typeof raw.source.color === 'string' ? { color: raw.source.color } : { wallpaper: null },
      blur: raw.blur !== false,
      scale: typeof raw.scale === 'number' ? Math.min(1.25, Math.max(0.5, raw.scale)) : 'dynamic',
    };
  } catch {
    return { ...defaults };
  }
};

export const saveOpts = (opts: ThemeOpts) => localStorage.setItem(KEY, JSON.stringify(opts));

const apply = (colors: Record<string, string>, mode: ThemeMode) => {
  const style = document.documentElement.style;
  for (const [name, value] of Object.entries(colors)) style.setProperty(`--md-sys-color-${name}`, value);
  const resolved = mode === 'auto' ? 'dark' : mode;
  document.documentElement.dataset.mode = resolved;
};

export function applyEffects(opts: ThemeOpts) {
  document.documentElement.classList.toggle('no-blur', !opts.blur);
  void applyScale(opts);
}

let systemScaleCache: number | null = null;

export async function applyScale(opts: ThemeOpts) {
  let factor = typeof opts.scale === 'number' ? opts.scale : systemScaleCache;
  if (factor == null) {
    systemScaleCache = isTauri ? await systemScale().catch(() => 1) : 1;
  }
  factor = typeof opts.scale === 'number' ? opts.scale : systemScaleCache! * 0.9;
  document.documentElement.style.zoom = String(Math.min(1.25, Math.max(0.5, factor)));
}

let lastApplied = '';
let lastPalette: { colors: Record<string, string>; mode: ThemeMode } | null = null;

export async function pushTheme(opts: ThemeOpts, force = false): Promise<boolean> {
  applyEffects(opts);
  const sig = JSON.stringify({ m: opts.mode, s: opts.scheme, c: opts.source });
  if (!isTauri) return false;
  if (sig === lastApplied && !force) return true;
  try {
    const colors = await setTheme({ mode: opts.mode, scheme: opts.scheme, source: opts.source });
    lastApplied = sig;
    lastPalette = { colors, mode: opts.mode };
    apply(colors, opts.mode);
    return true;
  } catch {
    return false;
  }
}

export async function initTheme() {
  const opts = loadOpts();
  applyEffects(opts);
  if (lastPalette) apply(lastPalette.colors, lastPalette.mode);
  if (!isTauri) return;
  const w = window as unknown as { __melissmTheme?: boolean };
  if (!w.__melissmTheme) {
    w.__melissmTheme = true;
    listen<Record<string, string>>('theme-changed', (e) => {
      lastPalette = { colors: e.payload, mode: loadOpts().mode };
      apply(e.payload, loadOpts().mode);
    });
    document.addEventListener('astro:after-swap', () => {
      const o = loadOpts();
      applyEffects(o);
      if (lastPalette) apply(lastPalette.colors, o.mode);
      void pushTheme(o, true);
    });
  }
  await pushTheme(opts);
}
