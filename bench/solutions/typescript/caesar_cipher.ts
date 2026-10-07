const text = "hello world";
let result = "";
for (const ch of text) {
  if (ch >= "a" && ch <= "z") {
    const shifted = (ch.charCodeAt(0) - 97 + 3) % 26;
    result += String.fromCharCode(97 + shifted);
  } else {
    result += ch;
  }
}
console.log(result);
