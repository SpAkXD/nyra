function isPalindrome(word: string): boolean {
  let i = 0;
  let j = word.length - 1;
  while (i < j) {
    if (word[i] !== word[j]) return false;
    i++;
    j--;
  }
  return true;
}

for (const word of ["level", "hello", "racecar", "robot", "a", "abba"]) {
  console.log(isPalindrome(word) ? "yes" : "no");
}
