type Pt = { x: number; y: number };

const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
const palette = () => ({
  dot: cssVar('--md-sys-color-on-surface'),
  primary: cssVar('--md-sys-color-primary'),
  onPrimary: cssVar('--md-sys-color-on-primary'),
  tertiary: cssVar('--md-sys-color-tertiary'),
});

const keyDirs: Record<string, Pt> = {
  w: { x: 0, y: -1 },
  s: { x: 0, y: 1 },
  a: { x: -1, y: 0 },
  d: { x: 1, y: 0 },
  ArrowUp: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
  ArrowLeft: { x: -1, y: 0 },
  ArrowRight: { x: 1, y: 0 },
};

const PITCH = 10;
const DOT = 8;
const SUB = 3;
const CELL = PITCH * SUB;

function makeCtx(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext('2d')!;
  const paintDot = (px: number, py: number, color: string, alpha: number) => {
    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.beginPath();
    if (ctx.roundRect) ctx.roundRect(px - DOT / 2, py - DOT / 2, DOT, DOT, 2.5);
    else ctx.rect(px - DOT / 2, py - DOT / 2, DOT, DOT);
    ctx.fill();
  };
  const dots = (fn: (x: number, y: number) => void) => {
    for (let gx = 0; gx < COLS_ALL; gx++) for (let gy = 0; gy < ROWS_ALL; gy++) fn(gx * PITCH + PITCH / 2, gy * PITCH + PITCH / 2);
  };
  const COLS_ALL = Math.round(canvas.width / PITCH);
  const ROWS_ALL = Math.round(canvas.height / PITCH);
  const drawField = (p: { dot: string }) => {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    dots((x, y) => paintDot(x, y, p.dot, 0.08));
  };
  const block = (cx: number, cy: number, color: string, alpha: number) => {
    for (let i = 0; i < SUB; i++) for (let j = 0; j < SUB; j++) paintDot((cx + i / SUB) * CELL + PITCH / 2, (cy + j / SUB) * CELL + PITCH / 2, color, alpha);
  };
  const single = (cx: number, cy: number, color: string, alpha: number) => paintDot(cx * CELL + CELL / 2, cy * CELL + CELL / 2, color, alpha);
  return { ctx, drawField, block, single };
}

export function mountSnake(canvas: HTMLCanvasElement, scoreEl: HTMLElement) {
  const { ctx, drawField, block } = makeCtx(canvas);
  const COLS = Math.floor(canvas.width / CELL);
  const ROWS = Math.floor(canvas.height / CELL);
  const dirs = [
    { x: 0, y: -1 },
    { x: 0, y: 1 },
    { x: -1, y: 0 },
    { x: 1, y: 0 },
  ];
  let snake: Pt[] = [];
  let dir: Pt = { x: 1, y: 0 };
  let pending: Pt | null = null;
  let food: Pt = { x: 0, y: 0 };
  let playing = false;
  let dead = false;
  let score = 0;
  let raf = 0;
  let last = 0;

  const inGrid = (x: number, y: number) => x >= 0 && y >= 0 && x < COLS && y < ROWS;
  const occupied = (x: number, y: number) => snake.some((s) => s.x === x && s.y === y);
  const placeFood = () => {
    do {
      food = { x: (Math.random() * COLS) | 0, y: (Math.random() * ROWS) | 0 };
    } while (occupied(food.x, food.y));
  };
  const reset = (auto: boolean) => {
    playing = !auto;
    dead = false;
    score = 0;
    snake = [
      { x: (COLS / 2) | 0, y: (ROWS / 2) | 0 },
      { x: (COLS / 2) | 0, y: (ROWS / 2) | 0 },
      { x: (COLS / 2) | 0, y: (ROWS / 2) | 0 },
    ];
    snake[1].x -= 1;
    snake[2].x -= 2;
    dir = { x: 1, y: 0 };
    pending = null;
    placeFood();
    scoreEl.classList.toggle('hidden', auto);
    scoreEl.textContent = '0';
  };
  const freeze = () => {
    playing = false;
    dead = true;
    scoreEl.classList.add('hidden');
  };
  const freeAround = (x: number, y: number) => dirs.filter((d) => inGrid(x + d.x, y + d.y) && !occupied(x + d.x, y + d.y)).length;
  const autoDir = () => {
    let best: Pt | null = null;
    let bestCost = Infinity;
    for (const d of dirs) {
      if (d.x === -dir.x && d.y === -dir.y) continue;
      const nx = snake[0].x + d.x;
      const ny = snake[0].y + d.y;
      if (!inGrid(nx, ny) || occupied(nx, ny)) continue;
      const dist = Math.abs(nx - food.x) + Math.abs(ny - food.y);
      const cost = dist - freeAround(nx, ny) * 0.55;
      if (cost < bestCost) {
        bestCost = cost;
        best = d;
      }
    }
    return best;
  };
  const step = () => {
    if (dead) return;
    if (!playing) {
      const d = autoDir();
      if (d) dir = d;
      else {
        reset(true);
        return;
      }
    } else if (pending) {
      dir = pending;
      pending = null;
    }
    const head = { x: snake[0].x + dir.x, y: snake[0].y + dir.y };
    if (!inGrid(head.x, head.y) || occupied(head.x, head.y)) {
      if (playing) freeze();
      else reset(true);
      return;
    }
    snake.unshift(head);
    if (head.x === food.x && head.y === food.y) {
      if (playing) {
        score++;
        scoreEl.textContent = String(score);
      }
      placeFood();
    } else snake.pop();
  };
  const draw = () => {
    const p = palette();
    drawField(p);
    block(food.x, food.y, p.tertiary, 1);
    snake.forEach((s, i) => block(s.x, s.y, p.primary, i === 0 ? 1 : Math.max(0.4, 1 - i * 0.03)));
  };
  const loop = (t: number) => {
    raf = requestAnimationFrame(loop);
    if (t - last < (playing ? 110 : 150)) return;
    last = t;
    step();
    draw();
  };
  const onKey = (e: KeyboardEvent) => {
    const el = e.target;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
      if (playing) reset(true);
      return;
    }
    if (!playing) return;
    const d = keyDirs[e.key];
    if (!d) return;
    e.preventDefault();
    if (!(d.x === -dir.x && d.y === -dir.y)) pending = d;
  };
  const onClick = () => {
    if (dead || !playing) reset(false);
  };
  const onVisible = () => {
    if (document.visibilityState === 'visible' && dead) reset(true);
  };
  canvas.addEventListener('click', onClick);
  document.addEventListener('keydown', onKey);
  document.addEventListener('visibilitychange', onVisible);
  reset(true);
  raf = requestAnimationFrame(loop);
  return () => {
    cancelAnimationFrame(raf);
    canvas.removeEventListener('click', onClick);
    document.removeEventListener('keydown', onKey);
    document.removeEventListener('visibilitychange', onVisible);
  };
}

export function mountShooter(canvas: HTMLCanvasElement, scoreEl: HTMLElement) {
  const { ctx, drawField, block, single } = makeCtx(canvas);
  const COLS = Math.floor(canvas.width / CELL);
  const ROWS = Math.floor(canvas.height / CELL);
  const W = canvas.width;
  const H = canvas.height;
  let ship: Pt = { x: COLS / 2, y: ROWS - 3 };
  let bullets: { x: number; y: number }[] = [];
  let enemies: Pt[] = [];
  let spawnAt = 0;
  let shootAt = 0;
  let playing = false;
  let dead = false;
  let score = 0;
  let raf = 0;
  let prev = 0;
  const held = new Set<string>();

  const reset = (auto: boolean) => {
    playing = !auto;
    dead = false;
    score = 0;
    ship = { x: COLS / 2, y: ROWS - 3 };
    bullets = [];
    enemies = [];
    spawnAt = 0;
    shootAt = 0;
    scoreEl.classList.toggle('hidden', auto);
    scoreEl.textContent = '0';
  };
  const freeze = () => {
    playing = false;
    dead = true;
    scoreEl.classList.add('hidden');
  };
  const shoot = (t: number) => {
    if (t > shootAt) {
      bullets.push({ x: ship.x, y: ship.y - 1 });
      shootAt = t + 240;
    }
  };
  const tick = (t: number) => {
    if (dead) {
      prev = t;
      return;
    }
    const dt = Math.min(0.05, (t - prev) / 1000 || 0.016);
    prev = t;
    const speed = playing ? 7 : 4.5;
    const fall = playing ? 3 : 2;
    if (!playing) {
      const target = enemies.reduce<Pt | null>((low, e) => (!low || e.y > low.y ? e : low), null);
      if (target) {
        const dx = target.x - ship.x;
        if (Math.abs(dx) > 0.08) ship.x += Math.sign(dx) * Math.min(Math.abs(dx), speed * dt);
        if (Math.abs(dx) < 0.7) shoot(t);
      } else {
        ship.x += Math.sin(t / 900) * dt * 2;
      }
    } else {
      let vx = 0;
      let vy = 0;
      if (held.has('a') || held.has('ArrowLeft')) vx -= 1;
      if (held.has('d') || held.has('ArrowRight')) vx += 1;
      if (held.has('w') || held.has('ArrowUp')) vy -= 1;
      if (held.has('s') || held.has('ArrowDown')) vy += 1;
      ship.x = Math.max(0.5, Math.min(COLS - 1.5, ship.x + vx * speed * dt));
      ship.y = Math.max(ROWS * 0.5, Math.min(ROWS - 1.5, ship.y + vy * speed * dt));
      shoot(t);
    }
    if (t > spawnAt) {
      enemies.push({ x: (Math.random() * (COLS - 1)) | 0, y: -0.5 });
      spawnAt = t + (playing ? 620 : 920);
    }
    bullets.forEach((b) => (b.y -= 14 * dt));
    bullets = bullets.filter((b) => b.y > -0.5);
    enemies.forEach((e) => (e.y += fall * dt));
    for (let i = enemies.length - 1; i >= 0; i--) {
      const e = enemies[i];
      const hitByBullet = bullets.some((b) => Math.abs(b.x - e.x) < 0.6 && Math.abs(b.y - e.y) < 0.9);
      const hitShip = Math.abs(e.x - ship.x) < 0.7 && Math.abs(e.y - ship.y) < 0.7;
      if (hitByBullet || e.y > ROWS - 1.6) {
        enemies.splice(i, 1);
        if (playing) {
          score++;
          scoreEl.textContent = String(score);
        }
      } else if (hitShip && playing) {
        freeze();
        return;
      }
    }
  };
  const draw = () => {
    const p = palette();
    drawField(p);
    for (const e of enemies) block(e.x, e.y, p.tertiary, 1);
    for (const b of bullets) single(b.x, b.y, p.primary, 1);
    block(ship.x, ship.y, p.onPrimary, 1);
    single(ship.x + 0.16, ship.y - 0.55, p.primary, 1);
  };
  const loop = (t: number) => {
    raf = requestAnimationFrame(loop);
    if (!prev) prev = t;
    tick(t);
    draw();
  };
  const onKey = (e: KeyboardEvent) => {
    const el = e.target;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
      if (playing) reset(true);
      return;
    }
    if (!playing) return;
    const d = keyDirs[e.key];
    if (!d) return;
    e.preventDefault();
    held.add(e.key);
  };
  const onKeyUp = (e: KeyboardEvent) => held.delete(e.key);
  const onClick = () => {
    if (dead || !playing) reset(false);
  };
  const onVisible = () => {
    if (document.visibilityState === 'visible' && dead) reset(true);
  };
  canvas.addEventListener('click', onClick);
  document.addEventListener('keydown', onKey);
  document.addEventListener('keyup', onKeyUp);
  document.addEventListener('visibilitychange', onVisible);
  reset(true);
  raf = requestAnimationFrame(loop);
  return () => {
    cancelAnimationFrame(raf);
    canvas.removeEventListener('click', onClick);
    document.removeEventListener('keydown', onKey);
    document.removeEventListener('keyup', onKeyUp);
    document.removeEventListener('visibilitychange', onVisible);
  };
}
