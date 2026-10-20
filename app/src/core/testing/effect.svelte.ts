// Runs `body` in an effect and counts its runs: tests use it to see what an effect depends on.

export function observe(body: () => void): { runs: () => number; stop: () => void } {
  let runs = 0;
  const stop = $effect.root(() => {
    $effect(() => {
      runs += 1;
      body();
    });
  });
  return { runs: () => runs, stop };
}
