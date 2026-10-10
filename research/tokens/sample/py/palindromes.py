words = ["level", "hello", "racecar", "robot", "a", "abba"]
for w in words:
    print("yes" if w == w[::-1] else "no")
