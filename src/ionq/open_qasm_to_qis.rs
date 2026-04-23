// This code is part of Qiskit.
//
// (C) Copyright IBM, IonQ 2025
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[repr(C)]
pub struct IonqTranslateResult {
    pub json_utf8: *mut c_char,
    pub error_utf8: *mut c_char,
}

unsafe extern "C" {
    fn translate_qasm3_to_ionq_qis_c(src: *const c_char) -> IonqTranslateResult;
    fn ionq_free_string(s: *mut c_char);
}

pub fn translate_qasm3_to_ionq_qis(src: &str) -> Result<String, String> {
    let c_src = CString::new(src).map_err(|_| "input contains interior NUL byte".to_string())?;

    let result = unsafe { translate_qasm3_to_ionq_qis_c(c_src.as_ptr()) };

    let output = unsafe {
        let json = if !result.json_utf8.is_null() {
            Some(CStr::from_ptr(result.json_utf8).to_string_lossy().into_owned())
        } else {
            None
        };

        let err = if !result.error_utf8.is_null() {
            Some(CStr::from_ptr(result.error_utf8).to_string_lossy().into_owned())
        } else {
            None
        };

        if !result.json_utf8.is_null() {
            ionq_free_string(result.json_utf8);
        }
        if !result.error_utf8.is_null() {
            ionq_free_string(result.error_utf8);
        }

        match (json, err) {
            (Some(j), None) => Ok(j),
            (_, Some(e)) => Err(e),
            _ => Err("wrapper returned neither output nor error".to_string()),
        }
    };

    output
}