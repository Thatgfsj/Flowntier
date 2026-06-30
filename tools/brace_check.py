#!/usr/bin/env python3
"""Quick brace-balance check for Rust source: strip strings + comments,
then walk { } and report final depth."""
import re
import sys

with open(sys.argv[1]) as f:
    src = f.read()

# Strip /* ... */ comments
src = re.sub(r"/\*.*?\*/", "", src, flags=re.DOTALL)
# Strip // ... EOL comments
src = re.sub(r"//[^\n]*", "", src)
# Strip raw strings r#"..."# (best-effort)
src = re.sub(r'r#".*?"#', '""', src, flags=re.DOTALL)
# Strip regular strings
src = re.sub(r'"(?:[^"\\]|\\.)*"', '""', src)

depth = 0
line = 1
last_zero_at = 0
for c in src:
    if c == "\n":
        line += 1
    elif c == "{":
        depth += 1
    elif c == "}":
        depth -= 1
        if depth == 0:
            last_zero_at = line

print(f"final depth = {depth}, last depth=0 at line {last_zero_at}")