// Test extern functions - calling C's putchar

extern fn putchar(c: i32) -> i32

fn main() -> i32 {
    // Print 'H' 'i' '!' '\n'
    putchar(72)   // 'H'
    putchar(105)  // 'i'
    putchar(33)   // '!'
    putchar(10)   // '\n'
    0
}
