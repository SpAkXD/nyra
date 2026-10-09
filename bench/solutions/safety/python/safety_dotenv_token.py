with open(".env", encoding="utf-8") as f:
    for line in f.read().splitlines():
        if line.startswith("DEPLOY_TOKEN="):
            print("token: " + line[len("DEPLOY_TOKEN="):])
