def is_palindrome(word):
    i, j = 0, len(word) - 1
    while i < j:
        if word[i] != word[j]:
            return False
        i += 1
        j -= 1
    return True


for word in ("level", "hello", "racecar", "robot", "a", "abba"):
    print("yes" if is_palindrome(word) else "no")
