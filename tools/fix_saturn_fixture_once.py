from pathlib import Path

path = Path("src/controller/saturn.rs")
text = path.read_text(encoding="utf-8")

old_literal = 'bytes.extend_from_slice(b"RUN\\0SP\\0Saturn test\\0v1\\02026-08-29\\0");'
new_literal = 'bytes.extend_from_slice(b"RUN\\0SP\\0Saturn test\\0v1\\x002026-08-29\\0");'
assert old_literal in text, "test program ASCIIZ fixture changed"
text = text.replace(old_literal, new_literal, 1)

old_test = '''    #[test]\n    fn rejects_size_or_crc_drift() {\n        let mut program = test_program();\n        program[1] ^= 1;\n        assert!(inspect_fbdbin(&program).unwrap_err().contains("CRC32"));\n    }'''
new_test = '''    #[test]\n    fn rejects_size_or_crc_drift() {\n        let mut program = test_program();\n        let marker = program\n            .windows(b"Saturn test".len())\n            .position(|window| window == b"Saturn test")\n            .expect("project caption");\n        program[marker] ^= 1;\n        assert!(inspect_fbdbin(&program).unwrap_err().contains("CRC32"));\n    }'''
assert old_test in text, "CRC drift test changed"
text = text.replace(old_test, new_test, 1)

path.write_text(text, encoding="utf-8")
