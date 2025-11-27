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
    loop {}
}
