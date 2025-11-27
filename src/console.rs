const UART0: *mut u8 = 0x0900_0000 as *mut u8;

unsafe fn putchar(c: u8) {
    // Write directly to UART data register
    unsafe {
        UART0.write_volatile(c);
    }
}

pub fn print(s: &str) {
    for c in s.bytes() {
        unsafe {
            putchar(c);
        }
    }
}
