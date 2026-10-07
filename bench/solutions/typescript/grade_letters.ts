function grade(score: number): string {
  if (score >= 90) return "A";
  if (score >= 80) return "B";
  if (score >= 70) return "C";
  if (score >= 60) return "D";
  return "F";
}

for (const score of [95, 90, 89, 75, 60, 59, 0]) {
  console.log(grade(score));
}
