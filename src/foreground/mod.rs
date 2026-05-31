pub mod active_window;
pub mod process_list;

pub use active_window::ForegroundDetector;
pub use process_list::{list_process_names, list_processes};
