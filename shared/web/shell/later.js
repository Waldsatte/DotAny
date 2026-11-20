// How a part calls a part made after it: the function is looked up on the shell's context (ctx) when it's called. The
// parts are made in the order the page is made in, and some call each other both ways (the side panels
// and the save card close each other), so a part names these at its top: const { closeSaveAs } = later(ctx).
export const later = (ctx) =>
  new Proxy(
    {},
    {
      get:
        (_, name) =>
        (...args) =>
          ctx[name](...args),
    }
  );
