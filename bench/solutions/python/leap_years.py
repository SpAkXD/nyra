def is_leap(year):
    return year % 4 == 0 and (year % 100 != 0 or year % 400 == 0)


for year in (1900, 1996, 2000, 2023, 2024, 2100):
    print("yes" if is_leap(year) else "no")
