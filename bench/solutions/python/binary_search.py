values = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37]


def search(target):
    low, high = 0, len(values) - 1
    while low <= high:
        mid = (low + high) // 2
        if values[mid] == target:
            return mid
        if values[mid] < target:
            low = mid + 1
        else:
            high = mid - 1
    return -1


for target in (23, 2, 37, 4):
    print(search(target))
