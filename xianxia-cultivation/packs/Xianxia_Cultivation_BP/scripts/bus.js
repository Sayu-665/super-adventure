// Tiny event bus so feature modules can react to each other without circular imports.
const handlers = new Map();

export function on(name, fn) {
  if (!handlers.has(name)) handlers.set(name, []);
  handlers.get(name).push(fn);
}

export function emit(name, ...args) {
  for (const fn of handlers.get(name) ?? []) {
    try {
      fn(...args);
    } catch (e) {
      console.warn(`[xian] handler for ${name} failed: ${e}`);
    }
  }
}
