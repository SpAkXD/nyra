const games = ["3 4 3 4 3 4 3", "1 2 2 3 3 4 3 4 4 6 4", "6 6 6 6 6 6", "2 3 4 5 1 2 3 4 7", "1 1 2 2 4 4 3 5 5", "4 3 3 2 2 1 2 1 1 5 1", "1 2 3 4 5 6 6 5"];

const ROWS = 5;
const COLS = 6;

function wins(b: string[][], r: number, c: number): boolean {
  const p = b[r][c];
  for (const [dr, dc] of [[0, 1], [1, 0], [1, 1], [1, -1]]) {
    let n = 1;
    for (const s of [1, -1]) {
      let rr = r + s * dr;
      let cc = c + s * dc;
      while (rr >= 0 && rr < ROWS && cc >= 0 && cc < COLS && b[rr][cc] === p) {
        n++;
        rr += s * dr;
        cc += s * dc;
      }
    }
    if (n >= 4) return true;
  }
  return false;
}

games.forEach((g, gi) => {
  const moves = g.split(" ").map(Number);
  const board = Array.from({ length: ROWS }, () => Array(COLS).fill("."));
  let result = "";
  for (let i = 0; i < moves.length; i++) {
    const p = i % 2 === 0 ? "X" : "O";
    const col = moves[i];
    let row = -1;
    if (col >= 1 && col <= COLS) for (let r = ROWS - 1; r >= 0; r--) if (board[r][col - 1] === ".") {
      row = r;
      break;
    }
    if (row < 0) {
      result = `illegal move ${i + 1} by ${p}`;
      break;
    }
    board[row][col - 1] = p;
    if (wins(board, row, col - 1)) {
      result = `${p} wins at move ${i + 1}`;
      break;
    }
  }
  if (result === "") {
    const full = board.every((r) => r.every((c) => c !== "."));
    result = full ? "draw" : `no winner after ${moves.length} moves`;
  }
  console.log(`game ${gi + 1}: ${result}`);
  for (const r of board) console.log(r.join(""));
});
