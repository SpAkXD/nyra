for (let t = 0; t < 15; t++) {
  const phase = t % 6;
  if (phase < 3) console.log("green");
  else if (phase === 3) console.log("yellow");
  else console.log("red");
}
