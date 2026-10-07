numbers = [4, 7, 1, 9, 6, 5, 2, 8]
target = 11
for i in range(len(numbers)):
    for j in range(i + 1, len(numbers)):
        if numbers[i] + numbers[j] == target:
            print(f"{i} {j}")
