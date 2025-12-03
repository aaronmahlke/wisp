// Test file for trait method completion with type parameter substitution
import std.ops.Add

struct Point {
    x: i32,
    y: i32,
}

// When typing "fn " inside this impl block,
// the LSP should suggest: fn add(self, rhs: Self) -> Self
// (not fn add(self, rhs: Rhs) -> Self)
impl Add for Point {
    // Type "fn " here and the autocomplete should show "add(self, rhs: Self) -> Self"
}

struct Vector {
    x: i32,
    y: i32,
}

// When typing "fn " inside this impl block with a type argument,
// the LSP should suggest: fn add(self, rhs: i32) -> Self
impl Add<i32> for Vector {
    // Type "fn " here and the autocomplete should show "add(self, rhs: i32) -> Self"
}


