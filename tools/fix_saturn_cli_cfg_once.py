from pathlib import Path

path = Path("src/main.rs")
text = path.read_text(encoding="utf-8")
old = 'fn parse_assignment(spec: &str) -> Result<(&str, i32), String> {'
new = '#[cfg(firmverse_saturn_native)]\nfn parse_assignment(spec: &str) -> Result<(&str, i32), String> {'
assert old in text, "parse_assignment signature changed"
path.write_text(text.replace(old, new, 1), encoding="utf-8")
