for (let row = 0; row < 6; row++) {
  let line = "";
  for (let col = 0; col < 6; col++) {
    line += (row + col) % 2 === 0 ? "#" : ".";
  }
  console.log(line);
}
