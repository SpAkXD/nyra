interface Rect {
  width: number;
  height: number;
}

const rects: Rect[] = [
  { width: 3, height: 4 },
  { width: 10, height: 2 },
  { width: 7, height: 7 },
];
for (const rect of rects) {
  const area = rect.width * rect.height;
  const perimeter = 2 * (rect.width + rect.height);
  console.log(`${area} ${perimeter}`);
}
