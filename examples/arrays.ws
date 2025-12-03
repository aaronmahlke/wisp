import std.io.{ print }

fn main() {
    // Array literal
    let numbers = [10, 20, 30, 40, 50];

    // Access by index with string interpolation
    let first = numbers[0];
    let middle = numbers[2];
    let last = numbers[4];
    print(&"First, middle, last: {first}, {middle}, {last}");

    // Iterate with for loop
    print(&"All elements:");
    for i in 0..5 {
        let n = numbers[i];
        print(&"  {n}");
    }

    // Sum of array elements
    let mut sum = 0;
    for i in 0..5 {
        sum = sum + numbers[i];
    }
    print(&"Sum: {sum}");
}
