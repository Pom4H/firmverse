//! Small WASM compiler package. Reuses the actual Rust compiler and validator;
//! there is no CPU emulator and no second binary serializer in this package.
#[path = "../../../src/controller/saturn.rs"]
pub mod saturn;
#[path = "../../../src/controller/saturn_compiler.rs"]
pub mod saturn_compiler;
use serde_json::json;
use std::cell::RefCell;
const MAX_INPUT: usize = 1_048_576;
thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}
/// Input is bounded and copied by the JS host before calling compile.
#[no_mangle]
pub extern "C" fn fv_input_reserve(length: usize) -> *mut u8 {
    if length > MAX_INPUT {
        return std::ptr::null_mut();
    }
    INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(length, 0);
        input.as_mut_ptr()
    })
}
#[no_mangle]
pub extern "C" fn fv_compile() -> i32 {
    let result=INPUT.with(|input| -> Result<serde_json::Value,String> {
        let input=input.borrow(); let text=std::str::from_utf8(&input).map_err(|_|"ControlIR must be UTF-8")?;
        let ir=saturn_compiler::parse_control_ir_json(text)?;
        let output=saturn_compiler::compile_control_ir(&ir)?;
        Ok(json!({"ok":true,"bytes":output.fbdbin,"elementCount":output.element_count,
            "screenCount":output.screen_count,"requiredRtl":output.required_rtl,
            "listing":output.listing.iter().map(|r|json!({"index":r.index,"id":r.id,"type":r.kind,"inputs":r.inputs,"params":r.params,"comment":r.comment})).collect::<Vec<_>>()}))
    });
    let ok = result.is_ok();
    let value = result.unwrap_or_else(|error| json!({"ok":false,"error":error}));
    OUTPUT.with(|output| *output.borrow_mut() = value.to_string().into_bytes());
    i32::from(ok)
}
#[no_mangle]
pub extern "C" fn fv_result_ptr() -> *const u8 {
    OUTPUT.with(|output| output.borrow().as_ptr())
}
#[no_mangle]
pub extern "C" fn fv_result_len() -> usize {
    OUTPUT.with(|output| output.borrow().len())
}

#[no_mangle]
pub extern "C" fn fv_inspect() -> i32 {
    let result = INPUT.with(|input| saturn::inspect_fbdbin(&input.borrow()));
    let ok = result.is_ok();
    let value = match result {
        Ok(info) => {
            json!({"ok":true,"elements":info.element_count,"rtl":info.required_rtl,"screens":info.screen_count})
        }
        Err(error) => json!({"ok":false,"error":error}),
    };
    OUTPUT.with(|output| *output.borrow_mut() = value.to_string().into_bytes());
    i32::from(ok)
}
