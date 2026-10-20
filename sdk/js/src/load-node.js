// Node.js (and Deno, Bun): fetch() cannot read file: URLs, so the bytes come from the disk.
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

export async function loadBytes(url) {
  if (url.protocol === "file:") return readFile(fileURLToPath(url));
  return fetch(url);
}
