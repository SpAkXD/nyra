text = "hello world"
result = ""
for ch in text:
    if "a" <= ch <= "z":
        result += chr((ord(ch) - ord("a") + 3) % 26 + ord("a"))
    else:
        result += ch
print(result)
