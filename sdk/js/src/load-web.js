// Browsers and workers: nyra.wasm is fetched (a bundler serves it as an asset next to the code).
export async function loadBytes(url) {
  return fetch(url);
}
