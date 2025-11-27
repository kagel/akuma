#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod boot;
mod console;

use alloc::{string::ToString, vec::Vec};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

static PROMPT: &str = "Akuma >: ";

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    allocator::init();

    let mut should_exit = false;
    while should_exit == false {
        console::print(PROMPT);

        let mut buffer = Vec::new();
        let len = console::read_line(&mut buffer, true);
        if len == 0 {
            continue;
        }
        if let Ok(text) = core::str::from_utf8(&buffer[..len]) {
            console::print("\n");
            match text.trim().to_lowercase().as_str() {
                "exit" => {
                    console::print_as_akuma("MEOWWWW!");
                    should_exit = true;
                }
                "meow" => {
                    console::print_as_akuma("Meow");
                }
                _ => {
                    console::print_as_akuma("pffft");
                }
            }
        }
    }

    // _start must never return (!) - hang forever
    loop {}
}
