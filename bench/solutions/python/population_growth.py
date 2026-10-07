population = 1000
for _ in range(10):
    population += population * 10 // 100
    population -= 50
    print(population)
