// Wisp Standard Library - I/O functions

import std.string

// Internal C FFI - not exported
extern fn putchar(c: i32) -> i32
extern fn fputs(s: str, stream: i64) -> i32
extern fn exit(code: i32) -> Never
extern static __stdoutp: i64
extern static __stderrp: i64

// Display trait - types that can be printed and converted to String
pub trait Display {
    fn to_string(&self) -> String
}

// Implement Display for i32
impl Display for i32 {
    fn to_string(&self) -> String {
        // Convert integer to string
        // Handle negative numbers
        if *self < 0 {
            let pos = 0 - *self;
            let pos_str = pos.to_string();
            String.from("-") + pos_str
        } else if *self == 0 {
            String.from("0")
        } else {
            // Build string from digits
            let mut result = String.new();
            let mut n = *self;
            let mut digits = String.new();
            
            while n > 0 {
                let digit = n % 10;
                let digit_char = (48 + digit) as i32;  // '0' + digit
                // We need to prepend, but we'll build reversed then... 
                // Actually simpler: just use the recursive approach
                n = n / 10;
            }
            
            // Simpler: use recursive helper or just call print to a buffer
            // For now, use a simple approach
            self.to_string_helper()
        }
    }
    
    fn to_string_helper(&self) -> String {
        if *self == 0 {
            String.from("0")
        } else if *self < 10 {
            let digit = *self % 10;
            let c = (48 + digit) as i32;
            // Create single char string
            let mut s = String.new();
            // We need a way to push a single char...
            // For now, use a lookup
            if digit == 0 { String.from("0") }
            else if digit == 1 { String.from("1") }
            else if digit == 2 { String.from("2") }
            else if digit == 3 { String.from("3") }
            else if digit == 4 { String.from("4") }
            else if digit == 5 { String.from("5") }
            else if digit == 6 { String.from("6") }
            else if digit == 7 { String.from("7") }
            else if digit == 8 { String.from("8") }
            else { String.from("9") }
        } else {
            let div = *self / 10;
            let digit = *self % 10;
            let prefix = div.to_string_helper();
            let suffix = digit.to_string_helper();
            prefix + suffix
        }
    }
}

// Implement Display for str
impl Display for str {
    fn to_string(&self) -> String {
        String.from(*self)
    }
}

// Implement Display for bool
impl Display for bool {
    fn to_string(&self) -> String {
        if *self {
            String.from("true")
        } else {
            String.from("false")
        }
    }
}

// Implement Display for String
impl Display for String {
    fn to_string(&self) -> String {
        // Return a copy of the string using the public clone method
        self.clone()
    }
}

// Generic print function - prints any type that implements Display, with newline
pub fn print<T: Display>(value: &T) {
    let s = value.to_string();
    let _ = fputs(s.ptr as str, __stdoutp);
    let _ = putchar(10);
}

// Panic function - prints message to stderr and exits
pub fn panic(msg: str) -> Never {
    let _ = fputs("panic: ", __stderrp);
    let _ = fputs(msg, __stderrp);
    let _ = putchar(10);
    exit(1)
}
