from pathlib import Path

path = Path("src/controller/saturn.rs")
text = path.read_text(encoding="utf-8")
old = '''use std::ffi::CStr;\nuse std::os::raw::{c_char, c_int};'''
new = '''#[cfg(firmverse_saturn_native)]\nuse std::ffi::CStr;\n#[cfg(firmverse_saturn_native)]\nuse std::os::raw::{c_char, c_int};'''
assert old in text, "Saturn FFI imports changed"
path.write_text(text.replace(old, new, 1), encoding="utf-8")
