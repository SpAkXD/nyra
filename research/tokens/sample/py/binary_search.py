def binary_search(arr, target):
    lo, hi = 0, len(arr) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if arr[mid] == target:
            return mid
        elif arr[mid] < target:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

arr = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37]
for v in [23, 2, 37, 4]:
    print(binary_search(arr, v))
