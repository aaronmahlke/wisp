// Wisp Standard Library - I/O functions

extern fn putchar(c: i32) -> i32
extern fn fputs(s: str, stream: i64) -> i32

// macOS: stdout is a global pointer called __stdoutp
extern static __stdoutp: i64

// Display trait - types that can be printed
trait Display {
    fn print(&self)
}

// Implement Display for i32
impl Display for i32 {
    fn print(&self) {
        if *self < 0 {
            let _ = putchar(45);  // '-'
            let pos = 0 - *self;
            pos.print();
        } else {
            if *self >= 10 {
                let div = *self / 10;
                div.print();
            }
            let digit = *self % 10;
            let _ = putchar(48 + digit);
        }
    }
}

// Implement Display for str
impl Display for str {
    fn print(&self) {
        let _ = fputs(*self, __stdoutp);
    }
}

// Implement Display for bool
impl Display for bool {
    fn print(&self) {
        if *self {
            let _ = fputs("true", __stdoutp);
        } else {
            let _ = fputs("false", __stdoutp);
        }
    }
}

// Generic print function - prints any type that implements Display
fn print<T: Display>(value: &T) {
    value.print();
}

// Print a newline
fn println() {
    let _ = putchar(10);
}
