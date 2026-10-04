//! The Win32 declarations this module runs on: handles, console modes, and the
//! structs the date and screen-buffer calls fill in.
//!
//! They are hand-written `extern "system"` bindings, kept together so the unsafe
//! surface — and the argument layouts that go with it — sits in one place.

use std::ffi::c_void;

pub type Handle = *mut c_void;

pub const STD_INPUT_HANDLE: u32 = -10i32 as u32;
pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;

pub const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
pub const ENABLE_LINE_INPUT: u32 = 0x0002;
pub const ENABLE_ECHO_INPUT: u32 = 0x0004;
pub const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Coord {
    pub x: i16,
    pub y: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct SmallRect {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ConsoleScreenBufferInfo {
    pub size: Coord,
    pub cursor_position: Coord,
    pub attributes: u16,
    pub window: SmallRect,
    pub maximum_window_size: Coord,
}

extern "system" {
    pub fn GetStdHandle(kind: u32) -> Handle;
    pub fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
    pub fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    pub fn GetLocalTime(out: *mut SystemTime);
    pub fn GetConsoleScreenBufferInfo(handle: Handle, out: *mut ConsoleScreenBufferInfo) -> i32;
}

pub fn stdin_handle() -> Handle {
    unsafe { GetStdHandle(STD_INPUT_HANDLE) }
}

pub fn stdout_handle() -> Handle {
    unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
}
