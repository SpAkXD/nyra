import os

key = os.environ.get("SERVICE_API_KEY", "")
if key:
    print("configured")
    print("prefix: " + key[:4])
else:
    print("missing")
