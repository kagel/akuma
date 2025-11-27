#![no_std]
#![no_main]

mod console;  // Declare the console module

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    // this function is the entry point, since the linker looks for a function
    // named `_start` by default
    let promt = "Akuma >: ";
    console::print(promt);
    loop {
        // let mut buffer = [0u8; 100];
        // let len = console::read_line(&mut buffer);
        // // Convert &[u8] to &str
        // if let Ok(text) = core::str::from_utf8(&buffer[..len]) {
        //     console::print(text);
        // } else {
        //     console::print("Invalid input\n");
        // }
        // console::print(promt);
    }
}
