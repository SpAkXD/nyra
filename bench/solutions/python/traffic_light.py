for t in range(15):
    phase = t % 6
    if phase < 3:
        print("green")
    elif phase == 3:
        print("yellow")
    else:
        print("red")
