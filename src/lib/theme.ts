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
const PALETTE_KEY = 'melissm.palette';

const defaults: ThemeOpts = { mode: 'auto', scheme: 'tonal-spot', source: { wallpaper: null }, blur: true, scale: 'dynamic' };

export const loadOpts = (): ThemeOpts => {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    const source =
      raw.source && typeof raw.source.color === 'string'
        ? { color: raw.source.color }
        : raw.source && 'artwork' in raw.source
          ? { artwork: null }
          : { wallpaper: null };
    return {
      mode: raw.mode === 'dark' || raw.mode === 'light' ? raw.mode : 'auto',
      scheme: typeof raw.scheme === 'string' ? raw.scheme : 'tonal-spot',
      source,
      blur: raw.blur !== false,
      scale: typeof raw.scale === 'number' ? Math.min(1.25, Math.max(0.5, raw.scale)) : 'dynamic',
    };
  } catch {
    return { ...defaults };
  }
};

export const saveOpts = (opts: ThemeOpts) => localStorage.setItem(KEY, JSON.stringify(opts));

const parseColor = (hex: string): [number, number, number, number] => {
  let h = hex.replace('#', '');
  if (h.length === 8) h = h.slice(2) + h.slice(0, 2);
  if (h.length === 4) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2] + h[3] + h[3];
  if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
  const n = parseInt(h.slice(0, 6), 16);
  const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) : 255;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255, a];
};

const toColor = ([r, g, b, a]: [number, number, number, number]) =>
  a >= 255 ? `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}` : `#${((r << 24) | (g << 16) | (b << 8) | a).toString(16).padStart(8, '0')}`;

const mixColor = (a: string, b: string, k: number) => {
  const [r1, g1, b1, a1] = parseColor(a);
  const [r2, g2, b2, a2] = parseColor(b);
  return toColor([
    Math.round(r1 + (r2 - r1) * k),
    Math.round(g1 + (g2 - g1) * k),
    Math.round(b1 + (b2 - b1) * k),
    Math.round(a1 + (a2 - a1) * k),
  ]);
};

const setVars = (colors: Record<string, string>) => {
  const style = document.documentElement.style;
  for (const [name, value] of Object.entries(colors)) style.setProperty(`--md-sys-color-${name}`, value);
};

let liveColors: Record<string, string> = {};
let animId = 0;

export function apply(colors: Record<string, string>, mode: ThemeMode, animate = false) {
  const target = mode === 'auto' ? 'dark' : mode;
  document.documentElement.dataset.mode = target;
  cancelAnimationFrame(animId);
  const animOn = animate && localStorage.getItem('melissm.anim') !== '0';
  const hasLive = Object.keys(liveColors).length > 0;
  if (!animOn || !hasLive) {
    setVars(colors);
    liveColors = { ...colors };
    localStorage.setItem(PALETTE_KEY, JSON.stringify(colors));
    return;
  }
  const from = { ...liveColors };
  const t0 = performance.now();
  const dur = 900;
  const step = (now: number) => {
    const k = Math.min(1, (now - t0) / dur);
    const e = 1 - Math.pow(1 - k, 4);
    const mixed: Record<string, string> = {};
    for (const [name, to] of Object.entries(colors)) {
      const a = from[name];
      mixed[name] = a ? mixColor(a, to, e) : to;
    }
    setVars(mixed);
    liveColors = mixed;
    if (k < 1) {
      animId = requestAnimationFrame(step);
    } else {
      localStorage.setItem(PALETTE_KEY, JSON.stringify(colors));
    }
  };
  animId = requestAnimationFrame(step);
}

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

let scaleTimer: ReturnType<typeof setTimeout> | undefined;
export function applyScaleSoon(opts: ThemeOpts) {
  const root = document.documentElement;
  root.classList.add('zooming');
  clearTimeout(scaleTimer);
  scaleTimer = setTimeout(() => {
    void applyScale(opts);
    scaleTimer = setTimeout(() => root.classList.remove('zooming'), 420);
  }, 240);
}

let lastApplied = '';
let pushSeq = 0;

export async function pushTheme(opts: ThemeOpts, force = false): Promise<boolean> {
  applyEffects(opts);
  const sig = JSON.stringify({ m: opts.mode, s: opts.scheme, c: opts.source });
  if (!isTauri) return false;
  if (sig === lastApplied && !force) return true;
  const seq = ++pushSeq;
  try {
    const colors = await setTheme({ mode: opts.mode, scheme: opts.scheme, source: opts.source });
    if (seq !== pushSeq) return false;
    lastApplied = sig;
    apply(colors, opts.mode, true);
    return true;
  } catch {
    return false;
  }
}

export async function initTheme() {
  const opts = loadOpts();
  applyEffects(opts);
  try {
    const saved = JSON.parse(localStorage.getItem(PALETTE_KEY) ?? '{}');
    if (saved && Object.keys(saved).length) apply(saved, opts.mode, false);
  } catch {}
  if (!isTauri) return;
  const w = window as unknown as { __melissmTheme?: boolean };
  if (!w.__melissmTheme) {
    w.__melissmTheme = true;
    listen<Record<string, string>>('theme-changed', (e) => {
      apply(e.payload, loadOpts().mode, true);
    });
    document.addEventListener('astro:after-swap', () => {
      const o = loadOpts();
      applyEffects(o);
      if (Object.keys(liveColors).length) apply(liveColors, o.mode, false);
      void pushTheme(o, true);
    });
  }
  await pushTheme(opts);
}
