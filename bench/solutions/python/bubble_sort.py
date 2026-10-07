numbers = [64, 34, 25, 12, 22, 11, 90]
n = len(numbers)
for i in range(n):
    for j in range(n - 1 - i):
        if numbers[j] > numbers[j + 1]:
            numbers[j], numbers[j + 1] = numbers[j + 1], numbers[j]
for value in numbers:
    print(value)
