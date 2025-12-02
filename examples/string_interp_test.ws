import "../std/io"
import "../std/string"

fn main() {
    // Test basic string interpolation
    let name = "World";
    let msg = "Hello, {name}!";
    print(&msg);

    // Test with integer
    let count = 42;
    print(&"Count is {count}");

    // Test with boolean
    let active = true;
    let msg3 = "Active: {active}";
    print(&msg3);

    // Test with multiple interpolations
    let x = 10;
    let y = 20;
    let msg4 = "Point: ({x}, {y})";
    print(&msg4);
}
