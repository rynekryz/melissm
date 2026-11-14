export const initSliders = () => {
  document.querySelectorAll<HTMLElement>('.xslider').forEach((el) => {
    if (el.dataset.ready) return;
    const input = el.querySelector<HTMLInputElement>('input[type="range"]');
    if (!input) return;
    el.dataset.ready = '1';
    const paint = () => {
      const min = +input.min || 0;
      const max = +input.max || 100;
      const ratio = (+input.value - min) / (max - min || 1);
      el.style.setProperty('--p', `${ratio * 100}%`);
      el.style.setProperty('--p-num', String(ratio));
    };
    input.addEventListener('input', paint);
    el.addEventListener('pointerdown', () => el.classList.add('dragging'));
    window.addEventListener('pointerup', () => el.classList.remove('dragging'), { passive: true });
    paint();
  });
};

export const initSwipe = () => {
  let row: HTMLElement | null = null;
  let startX = 0;
  let startScroll = 0;
  let lastX = 0;
  let lastT = 0;
  let vel = 0;
  let moved = false;
  let raf = 0;

  const glide = () => {
    if (!row) return;
    vel *= 0.94;
    row.scrollLeft -= vel;
    if (Math.abs(vel) > 0.4) {
      raf = requestAnimationFrame(glide);
    } else {
      row?.classList.remove('gliding');
      row = null;
    }
  };

  document.addEventListener(
    'pointerdown',
    (e) => {
      const el = (e.target as HTMLElement).closest<HTMLElement>('.h-row');
      if (!el) return;
      row = el;
      moved = false;
      vel = 0;
      cancelAnimationFrame(raf);
      row.classList.remove('gliding');
      startX = e.clientX;
      startScroll = el.scrollLeft;
      lastX = e.clientX;
      lastT = e.timeStamp;
    },
    { passive: true },
  );
  window.addEventListener(
    'pointermove',
    (e) => {
      if (!row) return;
      const dx = e.clientX - startX;
      if (Math.abs(dx) > 6) {
        moved = true;
        row.classList.add('dragging');
      }
      if (moved) {
        e.preventDefault();
        row.scrollLeft = startScroll - dx;
        const dt = e.timeStamp - lastT;
        if (dt > 0) vel = (e.clientX - lastX) / dt * 16;
        lastX = e.clientX;
        lastT = e.timeStamp;
      }
    },
    { passive: false },
  );
  window.addEventListener(
    'pointerup',
    () => {
      if (!row) return;
      const el = row;
      row = null;
      el.classList.remove('dragging');
      if (moved && Math.abs(vel) > 2) {
        el.classList.add('gliding');
        row = el;
        raf = requestAnimationFrame(glide);
      }
    },
    { passive: true },
  );
  document.addEventListener(
    'click',
    (e) => {
      if (moved) {
        e.stopPropagation();
        e.preventDefault();
        moved = false;
      }
    },
    true,
  );
};
