type Handler = () => void | Promise<void>;

const root = window as unknown as {
  __mmPageHandlers?: Map<string, Handler>;
  __mmPageBound?: boolean;
};

const norm = (path: string) => path.replace(/\/+$/, '') || '/';

const handlers = (root.__mmPageHandlers ??= new Map());

if (!root.__mmPageBound) {
  root.__mmPageBound = true;
  document.addEventListener('astro:page-load', () => {
    const page = document.querySelector('main')?.getAttribute('data-page');
    const handler = page ? handlers.get(norm(page)) : undefined;
    if (handler) void handler();
  });
}

export function onPage(path: string, fn: Handler) {
  handlers.set(norm(path), fn);
}
