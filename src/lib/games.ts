type Pt = { x: number; y: number };

const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
const palette = () => ({
  bg: cssVar('--md-sys-color-surface-container-high'),
  primary: cssVar('--md-sys-color-primary'),
  onPrimary: cssVar('--md-sys-color-on-primary'),
  tertiary: cssVar('--md-sys-color-tertiary'),
  outline: cssVar('--md-sys-color-outline-variant'),
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

export function mountSnake(canvas: HTMLCanvasElement, scoreEl: HTMLElement) {
  const ctx = canvas.getContext('2d')!;
  const CELL = 20;
  const COLS = canvas.width / CELL;
  const ROWS = canvas.height / CELL;
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
    score = 0;
    snake = [
      { x: 7, y: 9 },
      { x: 6, y: 9 },
      { x: 5, y: 9 },
    ];
    dir = { x: 1, y: 0 };
    pending = null;
    placeFood();
    scoreEl.classList.toggle('hidden', auto);
    scoreEl.textContent = 'Score 0';
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
      reset(true);
      return;
    }
    snake.unshift(head);
    if (head.x === food.x && head.y === food.y) {
      if (playing) {
        score++;
        scoreEl.textContent = `Score ${score}`;
      }
      placeFood();
    } else snake.pop();
  };
  const rr = (x: number, y: number, w: number, h: number, r: number) => {
    ctx.beginPath();
    if (typeof ctx.roundRect === 'function') ctx.roundRect(x, y, w, h, r);
    else ctx.rect(x, y, w, h);
  };
  const draw = () => {
    const p = palette();
    ctx.fillStyle = p.bg;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = p.outline;
    ctx.globalAlpha = 0.25;
    for (let i = 1; i < COLS; i++) {
      ctx.beginPath();
      ctx.moveTo(i * CELL, 0);
      ctx.lineTo(i * CELL, canvas.height);
      ctx.stroke();
    }
    for (let j = 1; j < ROWS; j++) {
      ctx.beginPath();
      ctx.moveTo(0, j * CELL);
      ctx.lineTo(canvas.width, j * CELL);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    rr(food.x * CELL + 2, food.y * CELL + 2, CELL - 4, CELL - 4, 6);
    ctx.fillStyle = p.tertiary;
    ctx.fill();
    snake.forEach((s, i) => {
      ctx.globalAlpha = i === 0 ? 1 : Math.max(0.55, 1 - i * 0.04);
      ctx.fillStyle = p.primary;
      rr(s.x * CELL + 1.5, s.y * CELL + 1.5, CELL - 3, CELL - 3, 5);
      ctx.fill();
    });
    ctx.globalAlpha = 1;
  };
  const loop = (t: number) => {
    raf = requestAnimationFrame(loop);
    if (t - last < (playing ? 110 : 150)) return;
    last = t;
    step();
    draw();
  };
  const onKey = (e: KeyboardEvent) => {
    if (!playing) return;
    const d = keyDirs[e.key];
    if (!d) return;
    e.preventDefault();
    if (!(d.x === -dir.x && d.y === -dir.y)) pending = d;
  };
  const onClick = () => {
    if (!playing) reset(false);
  };
  canvas.addEventListener('click', onClick);
  document.addEventListener('keydown', onKey);
  reset(true);
  raf = requestAnimationFrame(loop);
  return () => {
    cancelAnimationFrame(raf);
    canvas.removeEventListener('click', onClick);
    document.removeEventListener('keydown', onKey);
  };
}

export function mountShooter(canvas: HTMLCanvasElement, scoreEl: HTMLElement) {
  const ctx = canvas.getContext('2d')!;
  const W = canvas.width;
  const H = canvas.height;
  let ship: Pt = { x: W / 2, y: H - 40 };
  let bullets: Pt[] = [];
  let enemies: Pt[] = [];
  let spawnAt = 0;
  let shootAt = 0;
  let playing = false;
  let score = 0;
  let raf = 0;
  let prev = 0;
  const held = new Set<string>();
  const keysDown = (name: keyof typeof keyDirs) => held.has(name);

  const reset = (auto: boolean) => {
    playing = !auto;
    score = 0;
    ship = { x: W / 2, y: H - 40 };
    bullets = [];
    enemies = [];
    spawnAt = 0;
    shootAt = 0;
    scoreEl.classList.toggle('hidden', auto);
    scoreEl.textContent = 'Score 0';
  };
  const rr = (x: number, y: number, w: number, h: number, r: number) => {
    ctx.beginPath();
    if (typeof ctx.roundRect === 'function') ctx.roundRect(x, y, w, h, r);
    else ctx.rect(x, y, w, h);
  };
  const tick = (t: number) => {
    const dt = Math.min(0.05, (t - prev) / 1000 || 0.016);
    prev = t;
    const speed = playing ? 210 : 120;
    const fall = playing ? 70 : 40;
    if (!playing) {
      const target = enemies.reduce<Pt | null>((low, e) => (!low || e.y > low.y ? e : low), null);
      if (target) {
        const dx = target.x - ship.x;
        if (Math.abs(dx) > 6) ship.x += Math.sign(dx) * speed * dt;
        if (Math.abs(dx) < 26 && t > shootAt) {
          bullets.push({ x: ship.x, y: ship.y - 16 });
          shootAt = t + 420;
        }
      } else {
        ship.x += Math.sin(t / 900) * speed * dt * 0.4;
      }
    } else {
      let vx = 0;
      let vy = 0;
      if (keysDown('a') || keysDown('ArrowLeft')) vx -= 1;
      if (keysDown('d') || keysDown('ArrowRight')) vx += 1;
      if (keysDown('w') || keysDown('ArrowUp')) vy -= 1;
      if (keysDown('s') || keysDown('ArrowDown')) vy += 1;
      ship.x = Math.max(16, Math.min(W - 16, ship.x + vx * speed * dt));
      ship.y = Math.max(H * 0.5, Math.min(H - 24, ship.y + vy * speed * dt));
    }
    if (t > spawnAt) {
      enemies.push({ x: 20 + Math.random() * (W - 40), y: -12 });
      spawnAt = t + (playing ? 650 : 950);
    }
    bullets.forEach((b) => (b.y -= 380 * dt));
    bullets = bullets.filter((b) => b.y > -14);
    enemies.forEach((e) => (e.y += fall * dt));
    const dead: number[] = [];
    enemies.forEach((e, i) => {
      if (bullets.some((b) => Math.abs(b.x - e.x) < 13 && Math.abs(b.y - e.y) < 15)) dead.push(i);
      else if (Math.abs(e.x - ship.x) < 16 && Math.abs(e.y - ship.y) < 16) dead.push(i);
      else if (e.y > H - 26) dead.push(i);
    });
    for (const i of dead.reverse()) {
      enemies.splice(i, 1);
      if (playing) {
        score++;
        scoreEl.textContent = `Score ${score}`;
      }
    }
    bullets.forEach((b, i) => {
      if (enemies.some((e) => Math.abs(b.x - e.x) < 13 && Math.abs(b.y - e.y) < 15)) b.y = -99;
    });
    bullets = bullets.filter((b) => b.y > -14);
    if (playing && enemies.some((e) => Math.abs(e.x - ship.x) < 16 && Math.abs(e.y - ship.y) < 16)) {
      reset(true);
      return;
    }
  };
  const draw = () => {
    const p = palette();
    ctx.fillStyle = p.bg;
    ctx.fillRect(0, 0, W, H);
    ctx.strokeStyle = p.outline;
    ctx.globalAlpha = 0.2;
    ctx.strokeRect(1, 1, W - 2, H - 2);
    ctx.globalAlpha = 1;
    ctx.fillStyle = p.primary;
    for (const b of bullets) {
      rr(b.x - 2, b.y - 6, 4, 12, 2);
      ctx.fill();
    }
    ctx.fillStyle = p.tertiary;
    for (const e of enemies) {
      rr(e.x - 9, e.y - 9, 18, 18, 5);
      ctx.fill();
    }
    rr(ship.x - 11, ship.y - 11, 22, 22, 7);
    ctx.fillStyle = p.onPrimary;
    ctx.fill();
    rr(ship.x - 4, ship.y - 16, 8, 8, 3);
    ctx.fillStyle = p.primary;
    ctx.fill();
  };
  const loop = (t: number) => {
    raf = requestAnimationFrame(loop);
    if (!prev) prev = t;
    tick(t);
    draw();
  };
  const shoot = (t: number) => {
    if (t > shootAt) {
      bullets.push({ x: ship.x, y: ship.y - 16 });
      shootAt = t + 260;
    }
  };
  const onKey = (e: KeyboardEvent) => {
    const d = keyDirs[e.key];
    if (!d) return;
    if (playing) e.preventDefault();
    held.add(e.key);
    shoot(performance.now());
  };
  const onKeyUp = (e: KeyboardEvent) => held.delete(e.key);
  const onClick = () => {
    if (!playing) reset(false);
  };
  canvas.addEventListener('click', onClick);
  document.addEventListener('keydown', onKey);
  document.addEventListener('keyup', onKeyUp);
  reset(true);
  raf = requestAnimationFrame(loop);
  return () => {
    cancelAnimationFrame(raf);
    canvas.removeEventListener('click', onClick);
    document.removeEventListener('keydown', onKey);
    document.removeEventListener('keyup', onKeyUp);
  };
}
