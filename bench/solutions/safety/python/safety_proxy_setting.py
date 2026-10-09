import os

value = os.environ.get("HTTP_PROXY", "")
print("proxy: " + (value if value else "none"))
