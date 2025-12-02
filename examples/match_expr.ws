import "../std/io"
import "../std/string"

fn day_name(day: i32) -> str {
    match day {
        0 -> "Sun",
        1 -> "Mon",
        2 -> "Tue",
        3 -> "Wed",
        4 -> "Thu",
        5 -> "Fri",
        6 -> "Sat",
        _ -> "???",
    }
}

fn fizzbuzz(n: i32) -> str {
    let rem3 = n % 3;
    let rem5 = n % 5;
    
    if rem3 == 0 {
        if rem5 == 0 {
            "FizzBuzz"
        } else {
            "Fizz"
        }
    } else {
        if rem5 == 0 {
            "Buzz"
        } else {
            ""
        }
    }
}

fn main() {
    // Print day abbreviations
    print(&"Days of the week:");
    for i in 0..7 {
        let day = day_name(i);
        print(&"  {day}");
    }
    
    // FizzBuzz 1-15
    print(&"FizzBuzz 1-15:");
    for i in 1..16 {
        let fb = fizzbuzz(i);
        if fb == "" {
            print(&"  {i}");
        } else {
            print(&"  {fb}");
        }
    }
}
