use std::os::raw::{c_char, c_int, c_void};

pub const PKL_ERR_LOCK: c_int = 1;
pub const PKL_ERR_PROTOCOL: c_int = 2;

#[repr(C)]
pub struct pkl_error_t {
    pub message: *mut c_char,
}

#[repr(C)]
pub struct pkl_exec_t {
    _unused: [u8; 0],
}

#[allow(non_camel_case_types)]
pub type pkl_message_response_handler =
    unsafe extern "C" fn(length: c_int, message: *mut c_char, user_data: *mut c_void);

unsafe extern "C" {
    pub fn pkl_init(
        handler: pkl_message_response_handler,
        user_data: *mut c_void,
        exec: *mut *mut pkl_exec_t,
        error: *mut pkl_error_t,
    ) -> c_int;

    pub fn pkl_send_message(
        pexec: *mut pkl_exec_t,
        length: c_int,
        message: *mut c_char,
        error: *mut pkl_error_t,
    ) -> c_int;

    pub fn pkl_close(pexec: *mut pkl_exec_t, error: *mut pkl_error_t) -> c_int;

    pub fn pkl_version() -> *const c_char;
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, ensure};
    use std::ffi::CStr;
    use std::ptr;
    use std::sync::Mutex;

    // pkl_init can only be called once at a time.
    static LOCK: Mutex<()> = Mutex::new(());

    fn err() -> pkl_error_t {
        pkl_error_t {
            message: ptr::null_mut(),
        }
    }

    unsafe extern "C" fn noop_handler(_len: c_int, _msg: *mut c_char, _ud: *mut c_void) {}

    #[test]
    fn version() -> Result<()> {
        let v = unsafe { CStr::from_ptr(pkl_version()) }.to_str()?;
        ensure!(v.starts_with("0.30"), "unexpected version: {v}");
        Ok(())
    }

    #[test]
    fn init_close() -> Result<()> {
        let _guard = LOCK.lock().unwrap();
        let mut exec: *mut pkl_exec_t = ptr::null_mut();
        let mut e = err();

        let rc = unsafe { pkl_init(noop_handler, ptr::null_mut(), &mut exec, &mut e) };
        ensure!(rc == 0, "pkl_init failed: {rc}");
        ensure!(!exec.is_null(), "exec is null after pkl_init");

        // Only one instance allowed.
        let rc = unsafe { pkl_init(noop_handler, ptr::null_mut(), &mut exec, &mut e) };
        ensure!(rc != 0);
        ensure!(
            unsafe { CStr::from_ptr(e.message) }
                == c"pkl_init called multiple times without calling pkl_close"
        );

        let rc = unsafe { pkl_close(exec, &mut e) };
        ensure!(rc == 0, "pkl_close failed: {rc}");
        Ok(())
    }

    #[test]
    fn nulls() -> Result<()> {
        let mut e = err();
        let rc = unsafe { pkl_send_message(ptr::null_mut(), 0, ptr::null_mut(), &mut e) };
        ensure!(rc == -1);
        ensure!(unsafe { CStr::from_ptr(e.message) } == c"pexec is null");

        let mut e = err();
        let rc = unsafe { pkl_close(ptr::null_mut(), &mut e) };
        ensure!(rc == -1);
        ensure!(unsafe { CStr::from_ptr(e.message) } == c"pexec is null");
        Ok(())
    }

    #[test]
    fn roundtrip() -> Result<()> {
        use std::sync::mpsc;
        use std::time::Duration;

        unsafe extern "C" fn handler(length: c_int, message: *mut c_char, ud: *mut c_void) {
            let bytes =
                unsafe { std::slice::from_raw_parts(message as *const u8, length as usize) };
            let tx = unsafe { &*(ud as *const mpsc::Sender<Vec<u8>>) };
            tx.send(bytes.to_vec()).unwrap();
        }

        let _guard = LOCK.lock().unwrap();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        let mut exec: *mut pkl_exec_t = ptr::null_mut();
        let mut e = err();
        let rc = unsafe { pkl_init(handler, &tx as *const _ as *mut c_void, &mut exec, &mut e) };
        ensure!(rc == 0, "pkl_init failed: {rc}");

        // CreateEvaluator request: [0x20, {requestId, allowedModules, allowedResources}]
        use rmpv::Value;
        let msg = Value::Array(vec![
            Value::from(0x20u64),
            Value::Map(vec![
                (Value::from("requestId"), Value::from(1u64)),
                (
                    Value::from("allowedModules"),
                    Value::Array(vec![
                        Value::from("pkl:"),
                        Value::from("repl:"),
                        Value::from("file:"),
                    ]),
                ),
                (
                    Value::from("allowedResources"),
                    Value::Array(vec![Value::from("env:"), Value::from("prop:")]),
                ),
            ]),
        ]);
        let mut buf = Vec::new();
        rmpv::encode::write_value(&mut buf, &msg)?;

        let rc = unsafe {
            pkl_send_message(
                exec,
                buf.len() as c_int,
                buf.as_ptr() as *mut c_char,
                &mut e,
            )
        };
        ensure!(rc == 0, "pkl_send_message failed: {rc}");

        let response = rx
            .recv_timeout(Duration::from_secs(1))
            .context("no response")?;
        let val = rmpv::decode::read_value(&mut &response[..])?;
        let arr = val.as_array().context("response is not array")?;
        ensure!(
            arr[0].as_u64() == Some(0x21),
            "expected CreateEvaluatorResponse (0x21)"
        );

        let body = arr[1].as_map().context("response body is not map")?;
        let eval_id = body
            .iter()
            .find(|(k, _)| k.as_str() == Some("evaluatorId"))
            .context("missing evaluatorId")?;
        ensure!(eval_id.1.as_i64().is_some(), "evaluatorId is not int");

        let rc = unsafe { pkl_close(exec, &mut e) };
        ensure!(rc == 0, "pkl_close failed: {rc}");
        Ok(())
    }
}
