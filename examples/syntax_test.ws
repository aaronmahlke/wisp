import std.io.print

fn main() {
    // Test: semicolons optional for expression statements
    let x = 5;
    let y = 10;

    // Expression statement without semicolon (should highlight correctly)
    print(&"Testing")

    // Expression statement with semicolon (should also work)
    print(&"Also works");

    // Test: string interpolation highlighting
    let name = "Wisp";
    let version = 1;

    // The { and } should be highlighted as punctuation.special
    // The 'name' and 'version' inside should be highlighted as code
    print(&"Hello from {name} v{version}!");

    // Test: for loop (keyword highlighting)
    for i in 0..5 {
        print(&i);
    }

    // Test: match expression (keyword highlighting)
    let result = match x {
        5 -> "five",
        _ -> "other",
    };

    // Test: as cast (keyword highlighting)
    let casted = x as i64;
}
