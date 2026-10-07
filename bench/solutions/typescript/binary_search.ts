const values: number[] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];

function search(target: number): number {
  let low = 0;
  let high = values.length - 1;
  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if (values[mid] === target) return mid;
    if (values[mid] < target) low = mid + 1;
    else high = mid - 1;
  }
  return -1;
}

for (const target of [23, 2, 37, 4]) {
  console.log(search(target));
}
