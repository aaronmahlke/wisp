// Wisp Standard Library - I/O functions

extern fn putchar(c: i32) -> i32
extern fn fputs(s: String, stream: i64) -> i32

// macOS: stdout is a global pointer called __stdoutp
extern static __stdoutp: i64

// Print a string without newline
fn print(s: String) {
    let _ = fputs(s, __stdoutp);
}

// Alias for compatibility
fn print_str(s: String) {
    print(s);
}

// Print a string with newline
fn print_line(s: String) {
    print(s);
    println();
}

// Print a single character
fn print_char(c: i32) {
    let _ = putchar(c);
}

// Print a single digit (0-9)
fn print_digit(d: i32) {
    let _ = putchar(48 + d);  // '0' = 48
}

// Print an integer (handles negatives)
fn print_i32(n: i32) {
    if n < 0 {
        let _ = putchar(45);  // '-'
        print_i32(0 - n);
    } else {
        if n >= 10 {
            print_i32(n / 10);
        }
        print_digit(n % 10);
    }
}

// Print a newline
fn println() {
    let _ = putchar(10);  // '\n'
}

// Print a space
fn print_space() {
    let _ = putchar(32);  // ' '
}
