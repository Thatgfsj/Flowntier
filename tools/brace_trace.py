#!/usr/bin/env python3
"""Trace brace depth line by line."""
import re
import sys

path = sys.argv[1]
with open(path) as f:
    src = f.read()

# Strip block comments
src = re.sub(r"/\*.*?\*/", "", src, flags=re.DOTALL)
# Strip line comments
src = re.sub(r"//[^\n]*", "", src)
# Strip raw strings r#"..."#
src = re.sub(r'r#".*?"#', '""', src, flags=re.DOTALL)
# Strip normal strings (handle escaped quotes)
src = re.sub(r'"(?:[^"\\]|\\.)*"', '""', src)

depth = 0
line = 1
last_depth_zero = 0
min_depth = 0
min_line = 0
for c in src:
    if c == "\n":
        line += 1
    elif c == "{":
        depth += 1
    elif c == "}":
        depth -= 1
        if depth < min_depth:
            min_depth = depth
            min_line = line
    if depth == 0 and last_depth_zero != line:
        last_depth_zero = line

print(f"file: {path}")
print(f"final depth = {depth}, min depth = {min_depth} at line {min_line}")
print(f"last depth=0 at line {last_depth_zero}")