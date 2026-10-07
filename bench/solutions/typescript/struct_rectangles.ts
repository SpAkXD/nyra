interface Rect {
  width: number;
  height: number;
}

function area(rect: Rect): number {
  return rect.width * rect.height;
}

function perimeter(rect: Rect): number {
  return 2 * (rect.width + rect.height);
}

const rects: Rect[] = [
  { width: 3, height: 4 },
  { width: 10, height: 2 },
  { width: 7, height: 7 },
];
for (const rect of rects) {
  console.log(`${area(rect)} ${perimeter(rect)}`);
}
