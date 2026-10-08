const W = 8;
const H = 6;
let snake: [number, number][] = [[2, 2], [1, 2], [0, 2]];
let dir = "R";
const foods: [number, number][] = [[4, 2], [2, 2], [6, 4], [0, 5], [7, 0], [3, 3]];
let fi = 0;
let food: [number, number] | null = foods[0];
const moves = "RRDLURLDDRDLLLLLLURDRR";
const delta: Record<string, [number, number]> = { U: [0, -1], D: [0, 1], L: [-1, 0], R: [1, 0] };
const opp: Record<string, string> = { U: "D", D: "U", L: "R", R: "L" };
let score = 0;
let over = 0;
for (let k = 0; k < moves.length; k++) {
  const m = moves[k];
  if (m !== opp[dir]) dir = m;
  const [dx, dy] = delta[dir];
  const nx = snake[0][0] + dx;
  const ny = snake[0][1] + dy;
  const grows = food !== null && food[0] === nx && food[1] === ny;
  let dead = nx < 0 || nx >= W || ny < 0 || ny >= H;
  if (!dead) {
    const limit = grows ? snake.length : snake.length - 1;
    for (let i = 0; i < limit; i++) if (snake[i][0] === nx && snake[i][1] === ny) dead = true;
  }
  if (dead) { over = k + 1; break; }
  if (grows) {
    snake.unshift([nx, ny]);
    score++;
    food = null;
    for (fi = fi + 1; fi < foods.length; fi++) {
      const f = foods[fi];
      if (!snake.some((c) => c[0] === f[0] && c[1] === f[1])) { food = f; break; }
    }
  } else {
    snake.unshift([nx, ny]);
    snake.pop();
  }
}
console.log(over ? `game over at move ${over}` : "all moves done");
console.log(`score ${score}`);
console.log(`length ${snake.length}`);
for (let y = 0; y < H; y++) {
  let row = "";
  for (let x = 0; x < W; x++) {
    const idx = snake.findIndex((c) => c[0] === x && c[1] === y);
    if (idx === 0) row += "H";
    else if (idx > 0) row += "o";
    else if (food && food[0] === x && food[1] === y) row += "*";
    else row += ".";
  }
  console.log(row);
}
