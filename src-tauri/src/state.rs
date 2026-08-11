use std::process::Child;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

pub struct AppState {
    pub install_running: AtomicBool,
    pub install_cancel: AtomicBool,
    pub comfy_process: Mutex<Option<Child>>,
    pub comfy_url: Mutex<Option<String>>,
    pub comfy_output_path: Mutex<Option<String>>,
    pub comfy_last_error: Mutex<Option<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            install_running: AtomicBool::new(false),
            install_cancel: AtomicBool::new(false),
            comfy_process: Mutex::new(None),
            comfy_url: Mutex::new(None),
            comfy_output_path: Mutex::new(None),
            comfy_last_error: Mutex::new(None),
        }
    }
}
