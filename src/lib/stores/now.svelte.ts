/** Shared 1s ticker for age columns. */
export const now = $state({ ms: Date.now() });
setInterval(() => {
  now.ms = Date.now();
}, 1000);
